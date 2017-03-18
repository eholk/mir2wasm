use libc::c_char;
use error::*;
use rustc::mir::{Mir, Local};
use rustc::mir::{UnOp, BinOp, Literal, Lvalue, Operand, ProjectionElem, Rvalue, AggregateKind,
                 CastKind, StatementKind, TerminatorKind};
use rustc::dep_graph::DepNode;
use rustc::middle::const_val::ConstVal;
use rustc_const_math::{ConstInt, ConstIsize};
use rustc::ty::{self, TyCtxt, Ty, FnSig};
use rustc::ty::layout::{self, Layout, Size};
use rustc::ty::subst::Substs;
use rustc::hir::intravisit::{self, Visitor, FnKind, NestedVisitorMap};
use rustc::hir::{FnDecl, BodyId};
use rustc::hir::def_id::DefId;
use rustc::traits::Reveal;
use syntax::ast::{NodeId, IntTy, UintTy, FloatTy};
use syntax::codemap::Span;
use std::ffi::CString;
use std::ptr;
use std::collections::HashMap;
use std::cell::RefCell;
use binaryen;
use binaryen::*;
use binaryen::builder::ExpressionBuilder;
use binaryen::relooper::Relooper;
use monomorphize;
use binops::binaryen_op_for;
use traits;
use rustc_data_structures::indexed_vec::Idx;

#[derive(Debug, Clone)]
pub struct WasmTransOptions {
    pub optimize: bool,
    pub interpret: bool,
    pub print: bool,
    pub trace: bool,
    pub binary_output_path: Option<String>,
}

impl WasmTransOptions {
    pub fn new() -> WasmTransOptions {
        WasmTransOptions {
            optimize: false,
            interpret: false,
            print: true,
            trace: false,
            binary_output_path: None,
        }
    }
}

fn visit_krate<'g, 'tcx>(tcx: TyCtxt<'g, 'tcx, 'tcx>,
                         module: builder::Module,
                         entry_fn: Option<NodeId>)
                         -> builder::Module {
    let mut context: BinaryenModuleCtxt = BinaryenModuleCtxt::new(tcx, module, entry_fn);
    tcx.visit_all_item_likes_in_krate(DepNode::Mir, &mut context.as_deep_visitor());
    context.module
}

pub fn trans_crate<'a, 'tcx>(tcx: TyCtxt<'a, 'tcx, 'tcx>,
                             entry_fn: Option<NodeId>,
                             options: &WasmTransOptions)
                             -> Result<()> {

    let _ignore = tcx.dep_graph.in_ignore();

    if options.trace {
        binaryen::set_api_tracing(true);
    }

    let mut module = builder::Module::new();
    module.auto_drop();

    // TODO: allow for a configurable (or auto-detected) memory size
    module.set_memory(256);

    let mut module = visit_krate(tcx, module, entry_fn);
    assert!(module.is_valid(),
            "Internal compiler error: invalid generated module");

    // TODO: check which of the Binaryen optimization passes we want aren't on by default here.
    //       eg, removing unused functions and imports, minification, etc
    if options.optimize {
        module.optimize();
    }

    if options.trace {
        binaryen::set_api_tracing(false);
    }

    if options.print && !options.interpret {
        module.print();
    }

    if options.interpret {
        module.interpret();
    }

    for output in &options.binary_output_path {
        module.write_to_file(output).expect("error writing wasm file");
    }

    Ok(())
}

struct BinaryenModuleCtxt<'b, 'gcx: 'b + 'tcx, 'tcx: 'b> {
    tcx: TyCtxt<'b, 'gcx, 'tcx>,
    module: builder::Module,
    entry_fn: Option<NodeId>,
    fun_types: HashMap<ty::FnSig<'gcx>, BinaryenFunctionTypeRef>,
    fun_names: HashMap<(DefId, ty::FnSig<'gcx>), CString>,
    c_strings: Vec<CString>,
}

impl<'c, 'gcx: 'c + 'tcx, 'tcx: 'c> BinaryenModuleCtxt<'c, 'gcx, 'tcx> {
    fn new(tcx: TyCtxt<'c, 'gcx, 'tcx>,
           module: builder::Module,
           entry_fn: Option<NodeId>)
           -> BinaryenModuleCtxt<'c, 'gcx, 'tcx> {
        BinaryenModuleCtxt {
            tcx: tcx,
            module: module,
            entry_fn: entry_fn,
            fun_types: HashMap::new(),
            fun_names: HashMap::new(),
            c_strings: Vec::new(),
        }
    }
}

// The address in wasm linear memory where we store the stack pointer
// TODO: investigate where should the preferred location be
const STACK_POINTER_ADDRESS: i32 = 0;

impl<'e, 'tcx: 'e, 'h> Visitor<'h> for BinaryenModuleCtxt<'e, 'tcx, 'tcx> {
    fn nested_visit_map<'this>(&'this mut self) -> NestedVisitorMap<'this, 'h> {
        NestedVisitorMap::None
    }

    fn visit_fn(&mut self, fk: FnKind<'h>, fd: &'h FnDecl, b: BodyId, s: Span, id: NodeId) {
        let did = self.tcx.hir.local_def_id(id);

        let generics = self.tcx.item_generics(did);

        // don't translate generic functions yet
        if generics.types.len() + generics.parent_types as usize > 0 {
            return;
        }

        let mir = {
            self.tcx.maps.mir.borrow()[&did]
        };

        let sig = self.tcx.item_type(did).fn_sig();
        let sig = sig.skip_binder();
        {
            let mut ctxt = BinaryenFnCtxt {
                tcx: self.tcx,
                mir: mir,
                did: did,
                sig: &sig,
                func: self.module.create_func(),
                entry_fn: self.entry_fn,
                fun_types: &mut self.fun_types,
                fun_names: &mut self.fun_names,
                c_strings: &mut self.c_strings,
                checked_op_local: None,
                var_map: Vec::new(),
                temp_map: Vec::new(),
                ret_var: None,
            };

            ctxt.trans();
        }

        intravisit::walk_fn(self, fk, fd, b, s, id)
    }
}

struct BinaryenFnCtxt<'d, 'gcx: 'd + 'tcx, 'tcx: 'd, 'module> {
    tcx: TyCtxt<'d, 'gcx, 'tcx>,
    mir: &'d RefCell<Mir<'tcx>>,
    did: DefId,
    sig: &'d FnSig<'gcx>,
    func: builder::Fn<'module>,
    entry_fn: Option<NodeId>,
    fun_types: &'d mut HashMap<ty::FnSig<'gcx>, BinaryenFunctionTypeRef>,
    fun_names: &'d mut HashMap<(DefId, ty::FnSig<'gcx>), CString>,
    c_strings: &'d mut Vec<CString>,
    checked_op_local: Option<BinaryenIndex>,
    var_map: Vec<Option<usize>>,
    temp_map: Vec<Option<usize>>,
    ret_var: Option<usize>,
}

impl<'f, 'gcx: 'f + 'tcx, 'tcx: 'f, 'module: 'f> BinaryenFnCtxt<'f, 'gcx, 'tcx, 'module> {
    fn num_args(&self) -> usize {
        self.sig.inputs().len()
    }

    fn get_local_index(&self, i: usize) -> Option<usize> {
        debug!("fetching local {:?}", i);
        debug!("  vars: {:?}", self.var_map);
        debug!("  temps: {:?}", self.temp_map);
        if i == 0 {
            debug!("returning retvar");
            return self.ret_var;
        }
        let i = i - 1;
        if i < self.num_args() {
            debug!("returning function arg {}", i);
            return Some(i);
        }
        let i = i - self.num_args();
        if i < self.var_map.len() {
            debug!("returning {}th local: {:?}", i, self.var_map[i]);
            return self.var_map[i];
        }
        let i = i - self.var_map.len();
        assert!(i < self.temp_map.len());
        debug!("returning {}th temp: {:?}", i, self.temp_map[i]);
        return self.temp_map[i];
    }
}

impl<'f, 'tcx: 'f, 'module: 'f> BinaryenFnCtxt<'f, 'tcx, 'tcx, 'module> {
    /// This is the main entry point for MIR->wasm fn translation
    fn trans(&'module mut self) {
        let mir = self.mir.borrow();

        // Maintain a cache of translated monomorphizations and bail
        // if we've already seen this one.
        let fn_name_ptr;
        if self.fun_names.contains_key(&(self.did, self.sig.clone())) {
            return;
        } else {
            let fn_name = sanitize_symbol(&self.tcx.item_path_str(self.did));
            let fn_name = CString::new(fn_name).expect("");
            fn_name_ptr = fn_name.as_ptr();
            self.fun_names.insert((self.did, self.sig.clone()), fn_name);
        }

        debug!("translating fn {:?}", self.tcx.item_path_str(self.did));

        // Translate arg and ret tys to wasm
        for ty in self.sig.inputs() {
            self.func.add_arg(rust_ty_to_builder(ty).expect("arg type has no representation"));
        }
        let mut needs_ret_var = false;
        let ret_ty = self.sig.output();
        debug!("ret_ty is {:?}", ret_ty);
        let binaryen_ret = if !ret_ty.is_nil() && !ret_ty.is_never() {
            needs_ret_var = true;
            rust_ty_to_builder(ret_ty)
        } else {
            None
        };
        debug!("needs_ret_var = {:?}", needs_ret_var);

        // Create the wasm vars.
        // Params and vars form the list of locals, both sharing the same index space.

        // TODO(eholk): Use mir.local_decls directly rather than the two iterators.
        for mir_var in mir.vars_iter() {
            debug!("adding local {:?}: {:?}",
                   mir_var,
                   mir.local_decls[mir_var].ty);
            match rust_ty_to_builder(mir.local_decls[mir_var].ty) {
                Some(ty) => {
                    let var = self.func.create_local(ty).index();
                    self.var_map.push(Some(var))
                }
                None => self.var_map.push(None),
            }
        }

        for mir_var in mir.temps_iter() {
            debug!("adding temp {:?}", mir_var);
            let ty = rust_ty_to_builder(mir.local_decls[mir_var].ty)
                .map(|ty| self.func.create_local(ty).index());
            debug!("  type is {:?} ~> {:?}", mir.local_decls[mir_var].ty, &ty);
            self.temp_map.push(ty);
        }

        if needs_ret_var {
            debug!("adding ret var");
            self.ret_var = Some(self.func
                .create_local(rust_ty_to_builder(ret_ty)
                    .expect("return type has no representation"))
                .index());
        }

        // Function prologue: stack pointer local
        let stack_pointer_local = self.func.create_local(builder::ReprType::Int32).index();

        // checked operation local for the intermediate result of a checked operation (double-width)
        let checked_op_local = self.func.create_local(builder::ReprType::Int64).index();
        assert!(self.func.get_var(checked_op_local).ty() == builder::ReprType::Int64);
        self.checked_op_local = Some(checked_op_local.into());

        let locals_count = self.sig.inputs().len() + self.func.num_locals();
        debug!(concat!("{} wasm locals initially found - params: {}, vars: {} ",
                       "(incl. stack pointer helper ${}, ",
                       "checked operation helper ${})"),
               locals_count,
               self.sig.inputs().len(),
               self.func.num_vars(),
               stack_pointer_local.index(),
               checked_op_local.index());

        // Create the relooper for tying together basic blocks. We're
        // going to first translate the basic blocks without the
        // terminators, then go back over the basic blocks and use the
        // terminators to configure the relooper.
        let mut relooper = Relooper::new();

        debug!("{} MIR basic blocks to translate", mir.basic_blocks().len());

        for (i, bb) in mir.basic_blocks().iter().enumerate() {
            debug!("bb{}: {:#?}", i, bb);

            let mut binaryen_stmts = Vec::new();
            for stmt in &bb.statements {
                match stmt.kind {
                    StatementKind::Assign(ref lvalue, ref rvalue) => {
                        self.trans_assignment(lvalue, rvalue, &mut binaryen_stmts);
                    }
                    StatementKind::StorageLive(_) => {}
                    StatementKind::StorageDead(_) => {}
                    _ => panic!("{:?}", stmt.kind),
                }
            }

            let block_kind;

            // Some features of MIR terminators translate to wasm
            // expressions, some translate to relooper edges. These
            // are the expressions.
            match bb.terminator().kind {
                TerminatorKind::Return => {
                    // Emit function epilogue:
                    // TODO: like the prologue, not always necessary
                    unsafe {
                        debug!("emitting function epilogue, GetLocal({}) + Store",
                               (&stack_pointer_local).index());
                        let read_original_sp = self.func.get_local(stack_pointer_local);
                        let restore_original_sp = BinaryenStore(self.func.module.module,
                                                                4,
                                                                0,
                                                                0,
                                                                self.emit_sp().into(),
                                                                read_original_sp.into(),
                                                                BinaryenInt32());
                        binaryen_stmts.push(restore_original_sp);
                    }

                    debug!("emitting Return from fn {:?}",
                           self.tcx.item_path_str(self.did));
                    let expr = if ret_ty.is_nil() {
                        BinaryenExpressionRef(ptr::null_mut())
                    } else {
                        // Local 0 is guaranteed to be return pointer
                        self.trans_operand(&Operand::Consume(Lvalue::Local(Local::new(0))))
                    };
                    let expr = unsafe { BinaryenReturn(self.func.module.module, expr) };
                    binaryen_stmts.push(expr);
                    block_kind = BinaryenBlockKind::Default;
                }
                TerminatorKind::SwitchInt { ref discr, ref switch_ty, ref values, ref targets } => {
                    let discr = self.trans_operand(discr);
                    block_kind = BinaryenBlockKind::Switch(discr);
                }
                // TerminatorKind::Switch { ref discr, .. } => {
                //     let adt = self.trans_lval(discr).unwrap();
                //     let adt_ty = discr.ty(&*mir, self.tcx).to_ty(self.tcx);
                //
                //     if adt.offset.is_some() {
                //         panic!("unimplemented Switch with offset");
                //     }
                //
                //     let adt_layout = self.type_layout(adt_ty);
                //     let discr_val = match *adt_layout {
                //         Layout::General { discr, .. } => {
                //             let discr_size = discr.size().bytes() as u32;
                //             debug!("emitting GetLocal({}) + Load for ADT Switch condition",
                //                    adt.index.0);
                //             unsafe {
                //                 let ptr = BinaryenGetLocal(self.func.module.module,
                //                                            adt.index,
                //                                            BinaryenInt32());
                //                 BinaryenLoad(self.func.module.module,
                //                              discr_size,
                //                              0,
                //                              0,
                //                              0,
                //                              BinaryenInt32(),
                //                              ptr)
                //             }
                //         }
                //         Layout::CEnum { .. } => {
                //             debug!("emitting GetLocal({}) for CEnum Switch condition",
                //                    adt.index.0);
                //             unsafe {
                //                 BinaryenGetLocal(self.func.module.module,
                //                                  adt.index,
                //                                  BinaryenInt32())
                //             }
                //         }
                //         _ => panic!("unimplemented discrimant value for Layout {:?}",
                //                     adt_layout),
                //     };
                //
                //     block_kind = BinaryenBlockKind::Switch(discr_val);
                // }
                TerminatorKind::Call { ref func, ref args, ref destination, .. } => unsafe {
                    // NOTE: plan for the calling convention: i32/i64 f32/f64 are to be passed
                    // using the wasm stack and function parameters. For the other types, the
                    // manual stack in linear memory will be used, and pointers into this stack
                    // passed as i32s. A call to a function returning a struct will require
                    // preparing the output return value space on the caller function's frame, and
                    // the called function will write its return value there to avoid memcpys
                    if let Some((b_func, b_fnty, call_kind, is_never)) =
                        self.trans_fn_name_direct(func) {
                        let b_args: Vec<_> = args.iter().map(|a| self.trans_operand(a)).collect();
                        let b_call = match call_kind {
                            BinaryenCallKind::Direct => {
                                BinaryenCall(self.func.module.module,
                                             b_func,
                                             b_args.as_ptr(),
                                             BinaryenIndex(b_args.len() as _),
                                             b_fnty)
                            }
                            BinaryenCallKind::Import => {
                                BinaryenCallImport(self.func.module.module,
                                                   b_func,
                                                   b_args.as_ptr(),
                                                   BinaryenIndex(b_args.len() as _),
                                                   b_fnty)
                            }
                        };

                        match *destination {
                            Some((ref lvalue, _)) => {
                                if b_fnty == BinaryenNone() {
                                    // The result of the Rust call is put in MIR into a tmp local,
                                    // but the wasm function returns void (like the print externs)
                                    debug!("emitting {:?} Call to fn {:?} for unit type",
                                           call_kind,
                                           func);
                                    binaryen_stmts.push(b_call);
                                } else {
                                    let dest = self.trans_lval(lvalue)
                                        .expect("error translating lval");
                                    let dest_ty = lvalue.ty(&*mir, self.tcx).to_ty(self.tcx);
                                    let dest_layout = self.type_layout(dest_ty);

                                    match *dest_layout {
                                        Layout::Univariant { .. } |
                                        Layout::General { .. } => {
                                            // TODO: implement the calling convention for functions
                                            // returning non-primitive types FIXME: until then,
                                            // emit byte copies, which is inefficient but works for
                                            // now

                                            let dest_size = self.type_size(dest_ty) as i32 * 8;

                                            let tmp_dest = self.func
                                                .create_local(builder::ReprType::Int32)
                                                .index();

                                            debug!("tmp - emitting {:?} Call to fn {:?} + \
                                                    SetLocal({}) of the result pointer",
                                                   call_kind,
                                                   func,
                                                   tmp_dest);
                                            let set_local =
                                                BinaryenSetLocal(self.func.module.module,
                                                                 tmp_dest.into(),
                                                                 b_call);
                                            binaryen_stmts.push(set_local);

                                            debug!("tmp - allocating return value in linear \
                                                    memory to SetLocal({}), size: {:?}",
                                                   dest.index.0,
                                                   dest_size);
                                            let allocation =
                                                self.emit_alloca(dest.index, dest_size);
                                            binaryen_stmts.push(allocation);

                                            // TMP - the poor man's memcpy
                                            debug!("tmp - emitting Stores to copy result to \
                                                    stack frame");
                                            let ptr = BinaryenGetLocal(self.func.module.module,
                                                                       tmp_dest.into(),
                                                                       BinaryenInt32());
                                            let sp = self.emit_read_sp();
                                            let mut bytes_to_copy = dest_size;
                                            let mut offset = 0;
                                            while bytes_to_copy > 0 {
                                                let size = if bytes_to_copy >= 64 {
                                                    8
                                                } else if bytes_to_copy >= 32 {
                                                    4
                                                } else if bytes_to_copy >= 16 {
                                                    2
                                                } else {
                                                    1
                                                };

                                                let ty = if size == 8 {
                                                    BinaryenInt64()
                                                } else {
                                                    BinaryenInt32()
                                                };

                                                debug!("tmp - emitting Store copy, size: {}, \
                                                        offset: {}",
                                                       size,
                                                       offset);
                                                let read_bytes =
                                                    BinaryenLoad(self.func.module.module,
                                                                 size,
                                                                 0,
                                                                 offset,
                                                                 0,
                                                                 ty,
                                                                 ptr);
                                                let copy_bytes =
                                                    BinaryenStore(self.func.module.module,
                                                                  size,
                                                                  offset,
                                                                  0,
                                                                  self.emit_read_sp().into(),
                                                                  read_bytes,
                                                                  BinaryenInt64());
                                                binaryen_stmts.push(copy_bytes);

                                                bytes_to_copy -= size as i32 * 8;
                                                offset += size;
                                            }
                                        }

                                        Layout::Scalar { .. } |
                                        Layout::CEnum { .. } => {
                                            debug!("emitting {:?} Call to fn {:?} + SetLocal({}) \
                                                    of the result",
                                                   call_kind,
                                                   func,
                                                   dest.index.0);
                                            let set_local =
                                                BinaryenSetLocal(self.func.module.module,
                                                                 dest.index,
                                                                 b_call);
                                            binaryen_stmts.push(set_local);
                                        }

                                        _ => {
                                            panic!("unimplemented Call returned to Layout {:?}",
                                                   dest_layout)
                                        }
                                    }
                                }
                            }
                            _ => {
                                debug!("emitting Call to fn {:?}", func);
                                binaryen_stmts.push(b_call);
                                if is_never {
                                    debug!("{:?} is !, adding unreachable", func);
                                    let unreachable = self.unreachable().into();
                                    binaryen_stmts.push(unreachable);
                                }
                            }
                        }
                    } else {
                        panic!("untranslated fn call to {:?}", func)
                    }
                    block_kind = BinaryenBlockKind::Default;
                },
                TerminatorKind::Goto { .. } => {
                    block_kind = BinaryenBlockKind::Default;
                }
                TerminatorKind::Assert { .. } => {
                    block_kind = BinaryenBlockKind::Default;
                }
                _ => panic!("unimplemented terminator: {:?}", bb.terminator().kind),
            }
            let name = format!("bb{}", i);
            let name = CString::new(name).expect("");
            let name_ptr = name.as_ptr();
            self.c_strings.push(name);

            debug!("emitting {}-statement Block bb{}", binaryen_stmts.len(), i);

            let binaryen_expr = unsafe {
                BinaryenBlock(self.func.module.module,
                              name_ptr,
                              binaryen_stmts.as_ptr(),
                              BinaryenIndex(binaryen_stmts.len() as _))
            };
            match block_kind {
                BinaryenBlockKind::Default => {
                    add_block(&mut relooper, binaryen_expr);
                    // relooper.add_block_raw(binaryen_expr);
                }
                BinaryenBlockKind::Switch(ref cond) => {
                    relooper.add_block_raw_with_switch(*cond, binaryen_expr);
                }
            };
        }

        // Create the relooper edges from the bb terminators
        for (i, bb) in mir.basic_blocks().iter().enumerate() {
            debug!("{:?}", (i, bb));
            match bb.terminator().kind {
                TerminatorKind::Goto { ref target } => {
                    debug!("emitting Branch for Goto, from bb{} to bb{}",
                           i,
                           target.index());
                    relooper[i].add_goto(&relooper[target.index()]);
                }
                TerminatorKind::SwitchInt { ref discr, ref switch_ty, ref values, ref targets } => {
                    let from = &relooper[i];

                    let discr = self.trans_operand(discr);

                    assert!(values.len() > 0);

                    debug!("Adding edges for {}-way switch", values.len() + 1);

                    for j in 0..values.len() {
                        let value = values[j].to_u32().expect("invalid switch index").into();
                        let target = targets[j].index();
                        let value_ptr = &value;
                        debug!("Adding switch edge {}: bb{} -> bb{}",
                               values[j].to_u32().expect("invalid switch index"),
                               i,
                               target);
                        from.add_switch_case(value, &relooper[target]);
                    }

                    // Add the otherwise branch
                    debug!("Adding default switch from bb{} to bb{}",
                           i,
                           targets[targets.len() - 1].index());
                    let target_idx = targets[targets.len() - 1].index();
                    from.add_switch_default(&relooper[target_idx])
                }
                TerminatorKind::Return => {
                    // handled during bb creation
                }
                TerminatorKind::Assert { ref target, expected, ref cond, .. } => {
                    debug!("emitting Branch for Assert, from bb{} to bb{}. cond={:?}",
                           i,
                           target.index(),
                           cond);
                    let cond = self.trans_operand(cond);
                    // Add an unreachable for when the Assert fails.
                    //
                    // TODO(eholk): panic instead, with a helpful error message.
                    let panic = relooper.add_block(self.unreachable());
                    let panic = &relooper[panic];
                    let target = &relooper[target.index()];

                    let block = &relooper[i];
                    if expected {
                        block.add_cond_branch_raw(cond, target);
                        block.add_goto(panic);
                    } else {
                        block.add_cond_branch_raw(cond, panic);
                        block.add_goto(target);
                    }
                }
                TerminatorKind::Call { ref destination, ref cleanup, .. } => {
                    let _ = cleanup; // FIXME
                    match *destination {
                        Some((_, ref target)) => {
                            debug!("emitting Branch for Call, from bb{} to bb{}",
                                   i,
                                   target.index());
                            relooper[i].add_goto(&relooper[target.index()]);
                        }
                        _ => (),
                    }
                }
                _ => panic!("unimplemented terminator {:?}", bb.terminator().kind),
            }
        }

        if !self.fun_types.contains_key(self.sig) {
            let name = format!("rustfn-{}-{}", self.did.krate, self.did.index.as_u32());
            let name = CString::new(name).expect("");
            self.c_strings.push(name);
            let name = &self.c_strings[self.c_strings.len() - 1];
            let ty = self.func.create_sig_type(name, binaryen_ret);
            self.fun_types.insert(self.sig.clone(), ty);
        }

        let nid = self.tcx.hir.as_local_node_id(self.did).expect("");

        unsafe {
            if Some(self.did) == self.tcx.lang_items.panic_fn() {
                // TODO: when it's possible to print characters or interact with the environment,
                //       also handle #[lang = "panic_fmt"] to support panic messages
                debug!("emitting Unreachable function for panic lang item");
                // TODO(eholk): builderize this.
                let var_types = self.func.binaryen_var_types();
                BinaryenAddFunction(self.func.module.module,
                                    fn_name_ptr,
                                    *self.fun_types
                                        .get(self.sig)
                                        .expect("no type associated with function signature"),
                                    var_types.as_ptr(),
                                    var_types.len().into(),
                                    self.unreachable().into());
            } else {
                // Create the function prologue
                // TODO: the epilogue and prologue are not always necessary
                debug!("emitting function prologue, SetLocal({}) + Load",
                       (&stack_pointer_local).index());
                let copy_sp = self.func.set_local(stack_pointer_local, self.emit_read_sp());
                let prologue = relooper.add_block(copy_sp);
                if relooper.num_blocks() > 0 {
                    relooper[prologue].add_goto(&relooper[0])
                }
                let body = relooper.render(&mut self.func, prologue);

                // TODO(eholk): builderize this.
                let var_types = self.func.binaryen_var_types();
                BinaryenAddFunction(self.func.module.module,
                                    fn_name_ptr,
                                    *self.fun_types
                                        .get(self.sig)
                                        .expect("no type associated with function signature"),
                                    var_types.as_ptr(),
                                    var_types.len().into(),
                                    body);

                // TODO: don't unconditionally export this
                BinaryenAddExport(self.func.module.module, fn_name_ptr, fn_name_ptr);
            }

            if self.entry_fn == Some(nid) {
                let is_start = mir.arg_count == 2;
                let entry_fn_name = if is_start { "start" } else { "main" };
                let wasm_start = self.generate_runtime_start(&entry_fn_name);
                debug!("emitting wasm Start fn into entry_fn {:?}",
                       self.tcx.item_path_str(self.did));
                BinaryenSetStart(self.func.module.module, wasm_start);
            }
        }

        debug!("done translating fn {:?}\n",
               self.tcx.item_path_str(self.did));
    }

    fn trans_assignment(&mut self,
                        lvalue: &Lvalue<'tcx>,
                        rvalue: &Rvalue<'tcx>,
                        statements: &mut Vec<BinaryenExpressionRef>) {
        let mir = self.mir.borrow();

        let dest = match self.trans_lval(lvalue) {
            Some(dest) => dest,
            None => {
                // TODO: the rvalue may have some effects that we need to preserve. For example,
                // reading from memory can cause a fault.
                debug!("trans_assignment lval is unit: {:?} = {:?}; skipping",
                       lvalue,
                       rvalue);
                return;
            }
        };
        let dest_ty = lvalue.ty(&*mir, self.tcx).to_ty(self.tcx);

        let dest_layout = self.type_layout(dest_ty);

        match *rvalue {
            Rvalue::Use(ref operand) => {
                let src = self.trans_operand(operand);
                unsafe {
                    let statement = match dest.offset {
                        Some(offset) => {
                            debug!("emitting Store + GetLocal({}) for Assign Use '{:?} = {:?}'",
                                   dest.index.0,
                                   lvalue,
                                   rvalue);
                            let ptr = BinaryenGetLocal(self.func.module.module,
                                                       dest.index,
                                                       rust_ty_to_binaryen(dest_ty));
                            // TODO: match on the dest_ty to know how many bytes to write, not just
                            // i32s
                            BinaryenStore(self.func.module.module,
                                          4,
                                          offset,
                                          0,
                                          ptr,
                                          src,
                                          BinaryenInt32())
                        }
                        None => {
                            debug!("emitting SetLocal({}) for Assign Use '{:?} = {:?}'",
                                   dest.index.0,
                                   lvalue,
                                   rvalue);
                            BinaryenSetLocal(self.func.module.module, dest.index, src)
                        }
                    };
                    statements.push(statement);
                }
            }

            Rvalue::UnaryOp(ref op, ref operand) => {
                let operand = self.trans_operand(operand);
                unsafe {
                    let op = match *op {
                        UnOp::Not => BinaryenEqZInt32(),
                        _ => panic!("unimplemented UnOp: {:?}", op),
                    };
                    let op = BinaryenUnary(self.func.module.module, op, operand);
                    let statement = BinaryenSetLocal(self.func.module.module, dest.index, op);
                    statements.push(statement);
                }
            }

            Rvalue::BinaryOp(ref op, ref left, ref right) => {
                let src_ty = left.ty(&*self.mir.borrow(), self.tcx);
                let left = self.trans_operand(left);
                let right = self.trans_operand(right);

                unsafe {
                    // TODO: check if the dest_layout is signed or not (CEnum, etc)
                    // TODO: comparisons are signed only for now, so implement unsigned ones
                    let op = binaryen_op_for(*op, src_ty);

                    let op = BinaryenBinary(self.func.module.module, op, left, right);
                    let statement = match dest.offset {
                        Some(offset) => {
                            debug!("emitting Store + GetLocal({}) for Assign BinaryOp '{:?} = \
                                    {:?}'",
                                   dest.index.0,
                                   lvalue,
                                   rvalue);
                            let ptr = BinaryenGetLocal(self.func.module.module,
                                                       dest.index,
                                                       rust_ty_to_binaryen(dest_ty));
                            // TODO: match on the dest_ty to know how many bytes to write, not just
                            // i32s
                            BinaryenStore(self.func.module.module,
                                          4,
                                          offset,
                                          0,
                                          ptr,
                                          op,
                                          BinaryenInt32())
                        }
                        None => {
                            debug!("emitting SetLocal({}) for Assign BinaryOp '{:?} = {:?}'",
                                   dest.index.0,
                                   lvalue,
                                   rvalue);
                            BinaryenSetLocal(self.func.module.module, dest.index, op)
                        }
                    };
                    statements.push(statement);
                }
            }

            Rvalue::CheckedBinaryOp(ref op, ref left, ref right) => {
                // TODO: This shouldn't just be a copy BinaryOp above!
                // We should do some actual _checking_!

                let left = self.trans_operand(left);
                let right = self.trans_operand(right);

                unsafe {
                    // TODO: match on dest_ty.sty to implement binary ops for other types than just
                    // i32s
                    // TODO: check if the dest_layout is signed or not (CEnum, etc)
                    // TODO: operations are signed only for now, so implement unsigned ones
                    let op = match *op {
                        BinOp::Add => BinaryenAddInt64(),
                        BinOp::Sub => BinaryenSubInt64(),
                        BinOp::Mul => BinaryenMulInt64(),
                        BinOp::Div => BinaryenDivSInt64(),
                        _ => panic!("unimplemented BinOp: {:?}", op),
                    };

                    let op = BinaryenBinary(self.func.module.module,
                                            op,
                                            BinaryenUnary(self.func.module.module,
                                                          BinaryenExtendSInt32(),
                                                          left),
                                            BinaryenUnary(self.func.module.module,
                                                          BinaryenExtendSInt32(),
                                                          right));

                    let checked_local = self.checked_op_local.expect("no check op temporary");

                    statements.push(BinaryenSetLocal(self.func.module.module, checked_local, op));

                    let lower = BinaryenUnary(self.func.module.module,
                                              BinaryenWrapInt64(),
                                              BinaryenGetLocal(self.func.module.module,
                                                               checked_local,
                                                               BinaryenInt64()));

                    let thirty_two = self.int64(32);

                    let checked_op_local = self.checked_op_local.expect("no checked op temporary");
                    let upper =
                        BinaryenUnary(self.func.module.module,
                                      BinaryenWrapInt64(),
                                      BinaryenBinary(self.func.module.module,
                                                     BinaryenShrUInt64(),
                                                     BinaryenGetLocal(self.func.module.module,
                                                                      checked_op_local,
                                                                      BinaryenInt64()),
                                                     thirty_two.into()));

                    match dest.offset {
                        Some(offset) => {
                            debug!("emitting Store + GetLocal({}) for Assign Checked BinaryOp \
                                    '{:?} = {:?}'",
                                   dest.index.0,
                                   lvalue,
                                   rvalue);
                            let ptr = BinaryenGetLocal(self.func.module.module,
                                                       dest.index,
                                                       rust_ty_to_binaryen(dest_ty));
                            // TODO: match on the dest_ty to know how many bytes to write, not just
                            // i32s
                            statements.push(BinaryenStore(self.func.module.module,
                                                          4,
                                                          offset,
                                                          0,
                                                          ptr,
                                                          lower,
                                                          BinaryenInt32()));
                            statements.push(BinaryenStore(self.func.module.module,
                                                          4,
                                                          offset + 4,
                                                          0,
                                                          ptr,
                                                          upper,
                                                          BinaryenInt32()));
                        }
                        None => {
                            let dest_size = self.type_size(dest_ty) as i32 * 8;
                            // NOTE: use the variant's min_size and alignment for dest_size ?
                            debug!("allocating tuple in linear memory to SetLocal({}), size: \
                                    {:?} bytes",
                                   dest.index.0,
                                   dest_size);
                            let allocation = self.emit_alloca(dest.index, dest_size);
                            statements.push(allocation);
                            let ptr = BinaryenGetLocal(self.func.module.module,
                                                       dest.index,
                                                       rust_ty_to_binaryen(dest_ty));

                            statements.push(BinaryenStore(self.func.module.module,
                                                          4,
                                                          0,
                                                          0,
                                                          ptr,
                                                          lower,
                                                          BinaryenInt32()));
                            statements.push(BinaryenStore(self.func.module.module,
                                                          4,
                                                          4,
                                                          0,
                                                          ptr,
                                                          upper,
                                                          BinaryenInt32()));
                        }
                    }
                }
            }

            Rvalue::Ref(_, _, ref lvalue) => {
                // TODO: for shared refs only ?
                // TODO: works for refs to "our stack", but not the locals on the wasm stack yet
                let expr = self.trans_operand(&Operand::Consume(lvalue.clone()));
                unsafe {
                    debug!("emitting SetLocal({}) for Assign Ref '{:?} = {:?}'",
                           dest.index.0,
                           lvalue,
                           rvalue);
                    let expr = BinaryenSetLocal(self.func.module.module, dest.index, expr);
                    statements.push(expr);
                }
            }

            Rvalue::Aggregate(ref kind, ref operands) => {
                match *kind {
                    AggregateKind::Adt(ref adt_def, _, ref substs, _) => {
                        let dest_layout = self.type_layout_with_substs(dest_ty, substs);

                        // TODO: check sizes, alignments (abi vs preferred), etc
                        let dest_size = self.type_size_with_substs(dest_ty, substs) as i32 * 8;

                        match *dest_layout {
                            Layout::Univariant { ref variant, .. } => {
                                // NOTE: use the variant's min_size and alignment for dest_size ?
                                debug!("allocating struct '{:?}' in linear memory to \
                                        SetLocal({}), size: {:?} bytes ",
                                       adt_def,
                                       dest.index.0,
                                       dest_size);
                                let allocation = self.emit_alloca(dest.index, dest_size);
                                statements.push(allocation);

                                let offsets = ::std::iter::once(0)
                                    .chain(variant.offsets.iter().map(|s| s.bytes()));
                                debug!("emitting Stores for struct '{:?}' fields, values: {:?}",
                                       adt_def,
                                       operands);
                                self.emit_assign_fields(offsets, operands, statements);
                            }

                            Layout::General { discr, ref variants, .. } => {
                                if let AggregateKind::Adt(ref adt_def, variant, _, _) = *kind {
                                    let discr_val = match adt_def.variants[variant].discr {
                                        ty::VariantDiscr::Explicit(did) => unimplemented!(),
                                        ty::VariantDiscr::Relative(_) => unimplemented!(),
                                    };
                                    let discr_size = discr.size().bytes() as u32;

                                    debug!("allocating Enum '{:?}' in linear memory to \
                                            SetLocal({}), size: {:?} bytes",
                                           adt_def,
                                           dest.index.0,
                                           dest_size);
                                    let allocation = self.emit_alloca(dest.index, dest_size);
                                    statements.push(allocation);

                                    // set enum discr
                                    unsafe {
                                        debug!("emitting Store for Enum '{:?}' discr: {:?}",
                                               adt_def,
                                               discr_val);
                                        // BinaryenLiteralInt32(discr_val as i32));
                                        let discr_literal = BinaryenLiteralInt32(unimplemented!());
                                        let discr_val = BinaryenConst(self.func.module.module,
                                                                      discr_literal);
                                        let write_discr = BinaryenStore(self.func.module.module,
                                                                        discr_size,
                                                                        0,
                                                                        0,
                                                                        self.emit_read_sp().into(),
                                                                        discr_val,
                                                                        BinaryenInt32());
                                        statements.push(write_discr);
                                    }

                                    debug!("emitting Stores for Enum '{:?}' fields, operands \
                                            '{:?}'",
                                           adt_def,
                                           operands);
                                    let offsets = variants[variant]
                                        .offsets
                                        .iter()
                                        .map(|s| s.bytes());
                                    self.emit_assign_fields(offsets, operands, statements);
                                } else {
                                    panic!("tried to assign {:?} to Layout::General", kind);
                                }
                            }

                            Layout::CEnum { discr, .. } => {
                                assert_eq!(operands.len(), 0);
                                if let AggregateKind::Adt(adt_def, variant, _, _) = *kind {
                                    let discr_size = discr.size().bytes();
                                    if discr_size > 4 {
                                        panic!("unimplemented >32bit discr size: {}", discr_size);
                                    }

                                    // TODO: handle signed vs unsigned here as well, or just in the
                                    // BinOps ?
                                    let discr_val = adt_def.variants[variant].discr;
                                    let discr_val = unimplemented!(); //discr_val as i32;

                                    // set enum discr
                                    unsafe {
                                        debug!("emitting SetLocal({}) for CEnum Assign '{:?} = \
                                                {:?}', discr: {:?}",
                                               dest.index.0,
                                               lvalue,
                                               rvalue,
                                               discr_val);
                                        let discr_val =
                                            BinaryenConst(self.func.module.module,
                                                          BinaryenLiteralInt32(discr_val));
                                        let write_discr = BinaryenSetLocal(self.func.module.module,
                                                                           dest.index,
                                                                           discr_val);
                                        statements.push(write_discr);
                                    }
                                } else {
                                    panic!("tried to assign {:?} to Layout::CEnum", kind);
                                }
                            }

                            _ => {
                                panic!("unimplemented Assign Aggregate Adt {:?} on Layout {:?}",
                                       adt_def,
                                       dest_layout)
                            }
                        }
                    }

                    AggregateKind::Tuple => {
                        if operands.len() == 0 {
                            // TODO: in general, have a consistent strategy to handle the unit type
                            // assigns/returns
                            debug!("ignoring Assign '{:?} = {:?}'", lvalue, rvalue);
                        } else {
                            match *dest_layout {
                                Layout::Univariant { ref variant, .. } => {
                                    let dest_size = self.type_size(dest_ty) as i32 * 8;
                                    // NOTE: use the variant's min_size and alignment for
                                    // dest_size ?
                                    debug!("allocating tuple in linear memory to SetLocal({}), \
                                            size: {:?} bytes",
                                           dest.index.0,
                                           dest_size);
                                    let allocation = self.emit_alloca(dest.index, dest_size);
                                    statements.push(allocation);

                                    let offsets = ::std::iter::once(0).chain(variant.offsets
                                        .iter()
                                        .map(|s| s.bytes()));
                                    debug!("emitting Stores for tuple fields, values: {:?}",
                                           operands);
                                    self.emit_assign_fields(offsets, operands, statements);
                                }
                                _ => {
                                    panic!("unimplemented Tuple Assign '{:?} = {:?}'",
                                           lvalue,
                                           rvalue)
                                }
                            }
                        }
                    }

                    _ => panic!("unimplemented Assign Aggregate {:?}", kind),
                }
            }

            Rvalue::Cast(ref kind, ref operand, _) => {
                if dest.offset.is_some() {
                    panic!("unimplemented '{:?}' Cast with offset", kind);
                }

                match *kind {
                    CastKind::Misc => {
                        let src = self.trans_operand(operand);
                        let src_ty = operand.ty(&*mir, self.tcx);
                        let src_layout = self.type_layout(src_ty);

                        // TODO: handle more of the casts (miri doesn't really handle every Misc
                        // cast either right now)
                        match (src_layout, &dest_ty.sty) {
                            (&Layout::Scalar { .. }, &ty::TyInt(_)) |
                            (&Layout::Scalar { .. }, &ty::TyUint(_)) => unsafe {
                                debug!("emitting SetLocal({}) for Scalar Cast Assign '{:?} = \
                                        {:?}'",
                                       dest.index.0,
                                       lvalue,
                                       rvalue);
                                let copy_value =
                                    BinaryenSetLocal(self.func.module.module, dest.index, src);
                                statements.push(copy_value);
                            },
                            (&Layout::CEnum { .. }, &ty::TyInt(_)) |
                            (&Layout::CEnum { .. }, &ty::TyUint(_)) => unsafe {
                                debug!("emitting SetLocal({}) for CEnum Cast Assign '{:?} = {:?}'",
                                       dest.index.0,
                                       lvalue,
                                       rvalue);
                                let copy_discr =
                                    BinaryenSetLocal(self.func.module.module, dest.index, src);
                                statements.push(copy_discr);
                            },
                            _ => {
                                panic!("unimplemented '{:?}' Cast '{:?} = {:?}', for {:?} to {:?}",
                                       kind,
                                       lvalue,
                                       rvalue,
                                       src_layout,
                                       dest_ty.sty)
                            }
                        }
                    }
                    _ => {
                        panic!("unimplemented '{:?}' Cast '{:?} = {:?}'",
                               kind,
                               lvalue,
                               rvalue)
                    }
                }
            }

            _ => panic!("unimplemented Assign '{:?} = {:?}'", lvalue, rvalue),
        }
    }

    // TODO: handle > 2GB allocations, when more types are handled and there's a consistent story
    // around signed and unsigned
    fn emit_alloca(&mut self, dest: BinaryenIndex, dest_size: i32) -> BinaryenExpressionRef {
        let dest_size = self.int32(dest_size);
        let decr_sp = unsafe {
            BinaryenBinary(self.func.module.module,
                           BinaryenSubInt32(),
                           self.emit_read_sp().into(),
                           dest_size.into())
        };
        let decr_sp = builder::Expression::new(decr_sp, Some(builder::ReprType::Int32));
        let write_local = self.func.tee_local(dest.0 as usize, decr_sp);
        self.store(self.emit_sp(), write_local).into()
    }

    fn emit_sp(&self) -> builder::Expression {
        self.int32(STACK_POINTER_ADDRESS)
    }

    fn emit_read_sp(&self) -> builder::Expression {
        self.load(self.emit_sp(), builder::ReprType::Int32)
    }

    // TODO this function changed from being passed offsets-after-field to offsets-of-field...
    // but I suspect it still does the right thing - emit a store for every field.
    // Did it miss the first field and emit after the last field of the struct before?
    fn emit_assign_fields<I>(&mut self,
                             offsets: I,
                             operands: &[Operand<'tcx>],
                             statements: &mut Vec<BinaryenExpressionRef>)
        where I: IntoIterator<Item = u64>
    {
        unsafe {
            for (offset, operand) in offsets.into_iter().zip(operands) {
                let read_sp = self.emit_read_sp();
                // let operand_ty = mir.operand_ty(*self.tcx, operand);
                // TODO: match on the operand_ty to know how many bytes to store, not just i32s
                let src = self.trans_operand(operand);
                let write_field = BinaryenStore(self.func.module.module,
                                                4,
                                                offset as u32,
                                                0,
                                                read_sp.into(),
                                                src,
                                                BinaryenInt32());
                statements.push(write_field);
            }
        }
    }

    fn trans_lval(&mut self, lvalue: &Lvalue<'tcx>) -> Option<BinaryenLvalue> {
        let mir = self.mir.borrow();

        debug!("translating lval: {:?}", lvalue);

        let i = match *lvalue {
            Lvalue::Local(i) => {
                match self.get_local_index(i.index()) {
                    Some(i) => i as u32,
                    None => return None,
                }
            }
            Lvalue::Projection(ref projection) => {
                let base = match self.trans_lval(&projection.base) {
                    Some(base) => base,
                    None => return None,
                };
                let base_ty = projection.base.ty(&*mir, self.tcx).to_ty(self.tcx);
                let base_layout = self.type_layout(base_ty);

                match projection.elem {
                    ProjectionElem::Deref => {
                        if base.offset.is_none() {
                            // Converting the offset None into Some(0) tells calls this needs to be
                            // dereferenced.
                            return Some(BinaryenLvalue::new(base.index, Some(0),
                                        LvalueExtra::BaseTy(rust_ty_to_builder(base_ty)
                                        .expect("must dereference concrete type"))));
                        }
                        panic!("unimplemented Deref {:?}", lvalue);
                    }
                    ProjectionElem::Field(ref field, _) => {
                        let variant = match *base_layout {
                            Layout::Univariant { ref variant, .. } => variant,
                            Layout::General { ref variants, .. } => {
                                if let LvalueExtra::DowncastVariant(variant_idx) = base.extra {
                                    &variants[variant_idx]
                                } else {
                                    panic!("field access on enum had no variant index");
                                }
                            }
                            _ => panic!("unimplemented Field Projection: {:?}", projection),
                        };

                        let offset = variant.offsets[field.index()].bytes() as u32;
                        return Some(BinaryenLvalue::new(base.index,
                                                        base.offset,
                                                        LvalueExtra::None)
                            .offset(offset));
                    }
                    ProjectionElem::Downcast(_, variant) => {
                        match *base_layout {
                            Layout::General { discr, .. } => {
                                assert!(base.offset.is_none(),
                                        "unimplemented Downcast Projection with offset");

                                let offset = discr.size().bytes() as u32;
                                return Some(
                                    BinaryenLvalue::new(base.index, Some(offset),
                                                        LvalueExtra::DowncastVariant(variant)));
                            }
                            _ => panic!("unimplemented Downcast Projection: {:?}", projection),
                        }
                    }
                    _ => panic!("unimplemented Projection: {:?}", projection),
                }
            }
            _ => panic!("unimplemented Lvalue: {:?}", lvalue),
        };

        Some(BinaryenLvalue::new(BinaryenIndex(i), None, LvalueExtra::None))
    }

    fn trans_operand(&mut self, operand: &Operand<'tcx>) -> BinaryenExpressionRef {
        let mir = self.mir.borrow();

        match *operand {
            Operand::Consume(ref lvalue) => {
                let binaryen_lvalue = match self.trans_lval(lvalue) {
                    Some(lval) => lval,
                    None => {
                        debug!("operand lval is unit: {:?}", operand);
                        return self.unreachable().into();
                    }
                };
                let lval_ty = lvalue.ty(&*mir, self.tcx);
                let t = lval_ty.to_ty(self.tcx);
                let t = rust_ty_to_binaryen(t);

                unsafe {
                    match binaryen_lvalue.offset {
                        Some(offset) => {
                            debug!("emitting GetLocal({}) + Load for '{:?}'",
                                   binaryen_lvalue.index.0,
                                   lvalue);
                            let ptr = BinaryenGetLocal(self.func.module_ref(),
                                                       binaryen_lvalue.index,
                                                       BinaryenInt32());
                            // TODO(eholk): match on the field ty to know how many bytes to read,
                            // not just i32s
                            BinaryenLoad(self.func.module.module, 4, 0, offset, 0, t, ptr)
                        }
                        None => {
                            // debug!("emitting GetLocal for '{:?}'", lvalue);
                            BinaryenGetLocal(self.func.module_ref(), binaryen_lvalue.index, t)
                        }
                    }
                }
            }
            Operand::Constant(ref c) => {
                match c.literal {
                    Literal::Value { ref value } => {
                        // TODO: handle more Rust types here
                        unsafe {
                            let lit = match *value {
                                ConstVal::Integral(ConstInt::Isize(ConstIsize::Is32(val))) |
                                ConstVal::Integral(ConstInt::I32(val)) => BinaryenLiteralInt32(val),
                                ConstVal::Integral(ConstInt::I16(val)) => {
                                    BinaryenLiteralInt32(val as i32)
                                }
                                // TODO: Since we're at the wasm32 stage, and until wasm64, it's
                                // probably best if isize is always i32 ?
                                ConstVal::Integral(ConstInt::Isize(ConstIsize::Is64(val))) => {
                                    BinaryenLiteralInt32(val as i32)
                                }
                                ConstVal::Integral(ConstInt::I64(val)) => BinaryenLiteralInt64(val),
                                ConstVal::Bool(val) => {
                                    let val = if val { 1 } else { 0 };
                                    BinaryenLiteralInt32(val)
                                }
                                _ => panic!("unimplemented value: {:?}", value),
                            };
                            BinaryenConst(self.func.module.module, lit)
                        }

                    }
                    Literal::Promoted { .. } => panic!("unimplemented Promoted Literal: {:?}", c),
                    _ => panic!("unimplemented Constant Literal {:?}", c),
                }
            }
        }
    }


    fn trans_fn(&mut self,
                mut fn_did: DefId,
                substs: &Substs<'tcx>,
                sig: FnSig<'tcx>)
                -> (FnSig<'tcx>, DefId) {
        let is_trait_method = self.tcx.trait_of_item(fn_did).is_some();

        debug!("is_trait_method: {:?}", is_trait_method);

        let (substs, sig) = if !is_trait_method {
            debug!("Using original def_id: {:?}", fn_did);
            (substs, sig)
        } else {
            let (resolved_def_id, resolved_substs) =
                traits::resolve_trait_method(self.tcx, fn_did, substs);
            let ty = self.tcx.item_type(resolved_def_id);
            // TODO: investigate rustc trans use of
            // liberate_bound_regions or similar here
            let sig = ty.fn_sig();
            let sig = sig.skip_binder();

            fn_did = resolved_def_id;
            debug!("using alternate def_id: {:?}", fn_did);
            (resolved_substs, sig.clone())
        };

        let fn_sig = monomorphize::apply_substs(self.tcx, substs, &sig);

        // mark the fn defid seen to not have translated twice
        // TODO: verify this more thoroughly, works for our limited
        // tests right now
        if sig != fn_sig {
            let fn_name = sanitize_symbol(&self.tcx
                .item_path_str(fn_did));
            let fn_name = CString::new(fn_name).expect("");
            self.fun_names.insert((fn_did, sig.clone()), fn_name);
        }

        // This simple check is also done in trans() but doing it here
        // helps have a clearer debug log
        if !self.fun_names.contains_key(&(fn_did, fn_sig.clone())) {
            let map = self.tcx.maps.mir.borrow();
            let mir = map.get(&fn_did).expect("no mir map present");

            debug!("{:?}", line!());

            let mut ctxt = BinaryenFnCtxt {
                tcx: self.tcx,
                mir: mir,
                did: fn_did,
                sig: &fn_sig,
                func: self.func.module.create_func(),
                entry_fn: self.entry_fn,
                fun_types: &mut self.fun_types,
                fun_names: &mut self.fun_names,
                c_strings: &mut self.c_strings,
                checked_op_local: None,
                var_map: Vec::new(),
                temp_map: Vec::new(),
                ret_var: None,
            };

            debug!("translating monomorphized fn {:?}",
                   self.tcx.item_path_str(fn_did));
            ctxt.trans();
            debug!("done translating monomorphized {:?}, continuing translation of fn {:?}",
                   self.tcx.item_path_str(fn_did),
                   self.tcx.item_path_str(self.did));
        }

        debug!("{:?}", line!());

        return (fn_sig, fn_did);
    }

    fn trans_fn_name_direct(&mut self,
                            operand: &Operand<'tcx>)
                            -> Option<(*const c_char, BinaryenType, BinaryenCallKind, bool)> {
        debug!("translating {:?}", operand);
        match operand {
            &Operand::Constant(ref c) => {
                debug!("constant operand: {:?}", c);
                match c.literal {
                    Literal::Item { def_id, substs } => {
                        let ty = self.tcx.item_type(def_id);
                        if ty.is_fn() {
                            assert!(def_id.is_local());
                            let sig = ty.fn_sig();
                            let sig = sig.skip_binder();

                            let mut fn_did = def_id;
                            let fn_name = self.tcx.item_path_str(fn_did);
                            let fn_sig;
                            let mut call_kind = BinaryenCallKind::Direct;

                            debug!("fn_name = {:?}", fn_name);

                            // TODO(eholk): find out the correct way to recognize extern fns
                            if self.tcx.maps.mir.borrow().get(&fn_did).is_none() {
                                debug!("no mir map present, assuming fn is extern.");

                                assert!(fn_name != "wasm::::print_i32");

                                fn_sig = sig.clone();
                                call_kind = BinaryenCallKind::Import;
                                self.import_wasm_extern(fn_did, sig);
                            } else {

                                match fn_name.as_ref() {
                                    "wasm::::print_i32" |
                                    "wasm::::_print_i32" => {
                                        // extern wasm functions
                                        fn_sig = sig.clone();
                                        call_kind = BinaryenCallKind::Import;
                                        self.import_wasm_extern(fn_did, sig);
                                    }
                                    _ => {
                                        let (fn_sig_, fn_did_) =
                                            self.trans_fn(fn_did, substs, sig.clone());
                                        fn_sig = fn_sig_;
                                        fn_did = fn_did_;
                                    }
                                }
                            }
                            let ret_ty = if !fn_sig.output().is_nil() {
                                rust_ty_to_binaryen(fn_sig.output())
                            } else {
                                BinaryenNone()
                            };

                            let is_never = fn_sig.output().is_never() || fn_name == "panic";
                            Some((self.fun_names[&(fn_did, fn_sig)].as_ptr(),
                                  ret_ty,
                                  call_kind,
                                  is_never))
                        } else {
                            panic!("unimplemented ty {:?} for {:?}", ty, def_id);
                        }
                    }
                    _ => panic!("{:?}", c),
                }
            }
            _ => panic!(),
        }
    }

    fn generate_runtime_start(&mut self, entry_fn: &str) -> BinaryenFunctionRef {
        // runtime start fn
        let runtime_start_name = "__wasm_start";
        let runtime_export_name = "rust_entry";
        let runtime_start_name = CString::new(runtime_start_name).expect("");
        let runtime_start_name_ptr = runtime_start_name.as_ptr();
        self.c_strings.push(runtime_start_name);

        unsafe {
            let runtime_start_ty = BinaryenAddFunctionType(self.func.module.module,
                                                           runtime_start_name_ptr,
                                                           BinaryenNone(),
                                                           ptr::null_mut(),
                                                           BinaryenIndex(0));

            let mut statements = vec![];

            // set-up memory and stack
            // FIXME: decide how memory's going to work, the stack pointer address,
            //        track its initial size, etc and extract that into its own abstraction
            //     -> temporarily, just ask for one 64K page
            BinaryenSetMemory(self.func.module.module,
                              BinaryenIndex(1),
                              BinaryenIndex(1),
                              ptr::null(),
                              ptr::null(),
                              ptr::null(),
                              ptr::null(),
                              BinaryenIndex(0));

            let stack_top = self.int32(0xFFFF);
            let stack_init = BinaryenStore(self.func.module.module,
                                           4,
                                           0,
                                           0,
                                           self.emit_sp().into(),
                                           stack_top.into(),
                                           BinaryenInt32());
            statements.push(stack_init);

            // call start_fn(0, 0) or main()
            let entry_fn_name = &self.fun_names[&(self.did, self.sig.clone())];
            let entry_fn_call = if entry_fn == "start" {
                self.drop(self.call(entry_fn_name.to_str().expect("error converting string"),
                                    &[self.int32(0), self.int32(0)],
                                    Some(builder::ReprType::Int32)))
            } else {
                assert!(entry_fn == "main");
                assert!(self.sig.output().is_nil());
                self.call(entry_fn_name.to_str().expect("error converting string"),
                          &[],
                          None)
            };
            statements.push(entry_fn_call.into());

            let body = BinaryenBlock(self.func.module.module,
                                     ptr::null(),
                                     statements.as_ptr(),
                                     BinaryenIndex(statements.len() as _));

            BinaryenAddExport(self.func.module.module,
                              runtime_start_name_ptr,
                              runtime_export_name.as_ptr() as *const i8);
            BinaryenAddFunction(self.func.module.module,
                                runtime_start_name_ptr,
                                runtime_start_ty,
                                ptr::null_mut(),
                                BinaryenIndex(0),
                                body)
        }
    }

    fn import_wasm_extern(&mut self, did: DefId, sig: &ty::FnSig<'tcx>) {
        if self.fun_names.contains_key(&(did, sig.clone())) {
            return;
        }

        let full_name = self.tcx.item_path_str(did);
        debug!("adding extern for {:?}", full_name);

        // find the link attribute
        let parent_did =
            self.tcx.parent_def_id(did).expect("trying to import function with no parent");
        if self.tcx.has_attr(parent_did, "link") {

            let mut module_name = None;
            for attr in self.tcx.get_attrs(parent_did).iter().filter(|a| a.check_name("link")) {
                let items = match attr.meta_item_list() {
                    Some(item) => item,
                    None => continue,
                };

                let name = items.iter()
                    .find(|n| n.check_name("name"))
                    .and_then(|a| a.value_str())
                    .expect("missing link name");
                module_name = Some(name);
            }
            let module_name = &*module_name.expect("no module name found").as_str();

            let fn_name = &*self.tcx.item_name(did).as_str();

            debug!("importing {:?}::{:?}", module_name, fn_name);

            let full_name = CString::new(full_name).expect("error generating full name");
            let module_name = CString::new(module_name).expect("error generating module name");
            let fn_name = CString::new(fn_name).expect("error generating function name");
            unsafe {
                // TODO(eholk): support proper function types.
                let ty = BinaryenAddFunctionType(self.func.module.module,
                                                 full_name.as_ptr(),
                                                 BinaryenNone(),
                                                 ptr::null_mut(),
                                                 0u32.into());
                BinaryenAddImport(self.func.module.module,
                                  full_name.as_ptr(),
                                  module_name.as_ptr(),
                                  fn_name.as_ptr(),
                                  ty);
            }
            self.c_strings.push(fn_name);
            self.c_strings.push(full_name.clone());
            self.fun_names.insert((did, sig.clone()), full_name);

        } else {
            assert!(full_name == "wasm::::print_i32" || full_name == "wasm::::_print_i32");

            // import print i32
            let print_i32_name = CString::new("print_i32").expect("");
            let print_i32 = print_i32_name.as_ptr();
            self.fun_names.insert((did, sig.clone()), print_i32_name.clone());
            self.c_strings.push(print_i32_name);

            let spectest_module_name = CString::new("spectest").expect("");
            let spectest_module = spectest_module_name.as_ptr();
            self.c_strings.push(spectest_module_name);

            let print_fn_name = CString::new("print").expect("");
            let print_fn = print_fn_name.as_ptr();
            self.c_strings.push(print_fn_name);

            let print_i32_args = [BinaryenInt32()];
            unsafe {
                let print_i32_ty = BinaryenAddFunctionType(self.func.module.module,
                                                           print_i32,
                                                           BinaryenNone(),
                                                           print_i32_args.as_ptr(),
                                                           BinaryenIndex(1));
                BinaryenAddImport(self.func.module.module,
                                  print_i32,
                                  spectest_module,
                                  print_fn,
                                  print_i32_ty);
            }
        }
    }

    // Imported from miri and slightly modified to adapt to our monomorphize api
    fn type_layout_with_substs(&self, ty: Ty<'tcx>, substs: &Substs<'tcx>) -> &'tcx Layout {
        // TODO(solson): Is this inefficient? Needs investigation.
        let ty = monomorphize::apply_substs(self.tcx, substs, &ty);

        self.tcx.infer_ctxt((), Reveal::All).enter(|infcx| {
            // TODO(solson): Report this error properly.
            ty.layout(&infcx).expect("could not reveal inference context")
        })
    }

    #[inline]
    fn type_size(&self, ty: Ty<'tcx>) -> usize {
        let substs = Substs::empty();
        self.type_size_with_substs(ty, substs)
    }


    // Imported from miri
    #[inline]
    fn type_size_with_substs(&self, ty: Ty<'tcx>, substs: &'tcx Substs<'tcx>) -> usize {
        self.type_layout_with_substs(ty, substs).size(&self.tcx.data_layout).bytes() as usize
    }

    #[inline]
    fn type_layout(&self, ty: Ty<'tcx>) -> &'tcx Layout {
        let substs = Substs::empty();
        self.type_layout_with_substs(ty, substs)
    }
}

impl<'d, 'gcx: 'd + 'tcx, 'tcx: 'd, 'module> builder::ModuleOwned
    for BinaryenFnCtxt<'d, 'gcx, 'tcx, 'module> {
    fn module(&self) -> &builder::Module {
        self.func.module()
    }
}

fn rust_ty_to_binaryen<'tcx>(t: Ty<'tcx>) -> BinaryenType {
    // FIXME zero-sized-types
    if t.is_nil() || t.is_never() {
        return BinaryenNone();
    }

    match t.sty {
        ty::TyFloat(FloatTy::F32) => BinaryenFloat32(),
        ty::TyFloat(FloatTy::F64) => BinaryenFloat64(),
        ty::TyInt(IntTy::I64) |
        ty::TyUint(UintTy::U64) => BinaryenInt64(),
        // TODO: be explicit about all our types to avoid subtle bugs
        _ => BinaryenInt32(),
    }
}

fn rust_ty_to_builder<'tcx>(t: Ty<'tcx>) -> builder::Type {
    use binaryen::builder::ReprType::*;

    if t.is_nil() || t.is_never() {
        return None;
    } else {
        Some(match t.sty {
            ty::TyFloat(FloatTy::F32) => Float32,
            ty::TyFloat(FloatTy::F64) => Float64,
            ty::TyInt(IntTy::I64) |
            ty::TyUint(UintTy::U64) => Int64,
            _ => Int32,
        })
    }
}

fn sanitize_symbol(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '<' | '>' | ' ' | '(' | ')' => '_',
            _ => c,
        })
        .collect()
}

#[derive(Debug)]
enum BinaryenCallKind {
    Direct,
    Import, // Indirect // unimplemented at the moment
}

enum BinaryenBlockKind {
    Default,
    Switch(BinaryenExpressionRef),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
struct BinaryenLvalue {
    index: BinaryenIndex,
    offset: Option<u32>,
    extra: LvalueExtra,
}

impl BinaryenLvalue {
    fn new(index: BinaryenIndex, offset: Option<u32>, extra: LvalueExtra) -> Self {
        BinaryenLvalue {
            index: index,
            offset: offset,
            extra: extra,
        }
    }

    fn offset(&self, offset: u32) -> Self {
        let offset = match self.offset {
            None => Some(offset),
            Some(base_offset) => Some(base_offset + offset),
        };

        Self::new(self.index, offset, self.extra)
    }
}

// The following is imported from miri as well
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum LvalueExtra {
    None,
    /// the type of the thing this points to.
    BaseTy(builder::ReprType),
    // Length(u64),
    // TODO(solson): Vtable(memory::AllocId),
    DowncastVariant(usize),
}

trait IntegerExt {
    fn size(self) -> Size;
}

impl IntegerExt for layout::Integer {
    fn size(self) -> Size {
        use rustc::ty::layout::Integer::*;
        match self {
            I1 | I8 => Size::from_bits(8),
            I16 => Size::from_bits(16),
            I32 => Size::from_bits(32),
            I64 => Size::from_bits(64),
            I128 => panic!("i128 is not yet supported"),
        }
    }
}

fn add_block(relooper: &mut Relooper, body: BinaryenExpressionRef) {
    relooper.add_block_raw(body);
}
