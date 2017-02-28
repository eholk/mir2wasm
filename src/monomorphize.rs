use rustc::ty::subst::{Subst, Substs};
use rustc::ty::{FnSig, Ty, TyCtxt, TypeFoldable};
use rustc::infer::TransNormalize;

// pub fn apply_param_substs<'a, 'gcx, 'tcx>(tcx: TyCtxt<'a, 'gcx, 'tcx>,
//                                        param_substs: &Substs<'gcx>,
//                                        value: &FnSig<'gcx>)
//                                        -> FnSig<'gcx>
// {
//     let substituted = value.subst(tcx, param_substs);
//     substituted //tcx.normalize_associated_type(&substituted)
// }
