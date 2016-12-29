#![allow(dead_code, non_snake_case)]

use libc::{c_void, c_char, size_t};

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
pub struct BinaryenIndex(pub u32);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenType(pub u32);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenOp(pub u32);

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenModuleRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenFunctionTypeRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenExpressionRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenFunctionRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenImportRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BinaryenExportRef(pub *mut c_void);

#[repr(C)]
pub struct BinaryenLiteral {
    type_: i32,
    contents: i64,
}

#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct RelooperRef(pub *mut c_void);
#[repr(C)]
#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct RelooperBlockRef(pub *mut c_void);

macro_rules! safe_externs {
    ( $(pub fn $name:ident($($arg_name:ident: $arg_ty:ident), *) -> $rty:ident;) *) => {
        $(
            #[inline]
            pub fn $name($($arg_name: $arg_ty), *) -> $rty {
                extern {
                    pub fn $name($($arg_name: $arg_ty), *) -> $rty;
                }

                unsafe { $name($($arg_name), *) }
            }
        ) *
    }
}

safe_externs! {
    // Basic types
    pub fn BinaryenNone() -> BinaryenType;
    pub fn BinaryenInt32() -> BinaryenType;
    pub fn BinaryenInt64() -> BinaryenType;
    pub fn BinaryenFloat32() -> BinaryenType;
    pub fn BinaryenFloat64() -> BinaryenType;

    // Literals
    pub fn BinaryenLiteralInt32(x: i32) -> BinaryenLiteral;
    pub fn BinaryenLiteralInt64(x: i64) -> BinaryenLiteral;
    pub fn BinaryenLiteralFloat32(x: f32) -> BinaryenLiteral;
    pub fn BinaryenLiteralFloat64(x: f64) -> BinaryenLiteral;
    pub fn BinaryenLiteralFloat32Bits(x: i32) -> BinaryenLiteral;
    pub fn BinaryenLiteralFloat64Bits(x: i64) -> BinaryenLiteral;

    // Ops
    pub fn BinaryenClzInt32() -> BinaryenOp;
    pub fn BinaryenCtzInt32() -> BinaryenOp;
    pub fn BinaryenPopcntInt32() -> BinaryenOp;
    pub fn BinaryenNegFloat32() -> BinaryenOp;
    pub fn BinaryenAbsFloat32() -> BinaryenOp;
    pub fn BinaryenCeilFloat32() -> BinaryenOp;
    pub fn BinaryenFloorFloat32() -> BinaryenOp;
    pub fn BinaryenTruncFloat32() -> BinaryenOp;
    pub fn BinaryenNearestFloat32() -> BinaryenOp;
    pub fn BinaryenSqrtFloat32() -> BinaryenOp;
    pub fn BinaryenEqZInt32() -> BinaryenOp;
    pub fn BinaryenClzInt64() -> BinaryenOp;
    pub fn BinaryenCtzInt64() -> BinaryenOp;
    pub fn BinaryenPopcntInt64() -> BinaryenOp;
    pub fn BinaryenNegFloat64() -> BinaryenOp;
    pub fn BinaryenAbsFloat64() -> BinaryenOp;
    pub fn BinaryenCeilFloat64() -> BinaryenOp;
    pub fn BinaryenFloorFloat64() -> BinaryenOp;
    pub fn BinaryenTruncFloat64() -> BinaryenOp;
    pub fn BinaryenNearestFloat64() -> BinaryenOp;
    pub fn BinaryenSqrtFloat64() -> BinaryenOp;
    pub fn BinaryenEqZInt64() -> BinaryenOp;
    pub fn BinaryenExtendSInt32() -> BinaryenOp;
    pub fn BinaryenExtentUInt32() -> BinaryenOp;
    pub fn BinaryenWrapInt64() -> BinaryenOp;
    pub fn BinaryenTruncSFloat32ToInt32() -> BinaryenOp;
    pub fn BinaryenTruncSFloat32ToInt64() -> BinaryenOp;
    pub fn BinaryenTruncUFloat32ToInt32() -> BinaryenOp;
    pub fn BinaryenTruncUFloat32ToInt64() -> BinaryenOp;
    pub fn BinaryenTruncSFloat64ToInt32() -> BinaryenOp;
    pub fn BinaryenTruncSFloat64ToInt64() -> BinaryenOp;
    pub fn BinaryenTruncUFloat64ToInt32() -> BinaryenOp;
    pub fn BinaryenTruncUFloat64ToInt64() -> BinaryenOp;
    pub fn BinaryenReinterpretFloat32() -> BinaryenOp;
    pub fn BinaryenReinterpretFloat64() -> BinaryenOp;
    pub fn BinaryenConvertSInt32ToFloat32() -> BinaryenOp;
    pub fn BinaryenConvertSInt32ToFloat64() -> BinaryenOp;
    pub fn BinaryenConvertUInt32ToFloat32() -> BinaryenOp;
    pub fn BinaryenConvertUInt32ToFloat64() -> BinaryenOp;
    pub fn BinaryenConvertSInt64ToFloat32() -> BinaryenOp;
    pub fn BinaryenConvertSInt64ToFloat64() -> BinaryenOp;
    pub fn BinaryenConvertUInt64ToFloat32() -> BinaryenOp;
    pub fn BinaryenConvertUInt64ToFloat64() -> BinaryenOp;
    pub fn BinaryenPromoteFloat32() -> BinaryenOp;
    pub fn BinaryenDemoteFloat64() -> BinaryenOp;
    pub fn BinaryenReinterpretInt32() -> BinaryenOp;
    pub fn BinaryenReinterpretInt64() -> BinaryenOp;
    pub fn BinaryenAddInt32() -> BinaryenOp;
    pub fn BinaryenSubInt32() -> BinaryenOp;
    pub fn BinaryenMulInt32() -> BinaryenOp;
    pub fn BinaryenDivSInt32() -> BinaryenOp;
    pub fn BinaryenDivUInt32() -> BinaryenOp;
    pub fn BinaryenRemSInt32() -> BinaryenOp;
    pub fn BinaryenRemUInt32() -> BinaryenOp;
    pub fn BinaryenAndInt32() -> BinaryenOp;
    pub fn BinaryenOrInt32() -> BinaryenOp;
    pub fn BinaryenXorInt32() -> BinaryenOp;
    pub fn BinaryenShlInt32() -> BinaryenOp;
    pub fn BinaryenShrUInt32() -> BinaryenOp;
    pub fn BinaryenShrSInt32() -> BinaryenOp;
    pub fn BinaryenRotLInt32() -> BinaryenOp;
    pub fn BinaryenRotRInt32() -> BinaryenOp;
    pub fn BinaryenEqInt32() -> BinaryenOp;
    pub fn BinaryenNeInt32() -> BinaryenOp;
    pub fn BinaryenLtSInt32() -> BinaryenOp;
    pub fn BinaryenLtUInt32() -> BinaryenOp;
    pub fn BinaryenLeSInt32() -> BinaryenOp;
    pub fn BinaryenLeUInt32() -> BinaryenOp;
    pub fn BinaryenGtSInt32() -> BinaryenOp;
    pub fn BinaryenGtUInt32() -> BinaryenOp;
    pub fn BinaryenGeSInt32() -> BinaryenOp;
    pub fn BinaryenGeUInt32() -> BinaryenOp;
    pub fn BinaryenAddInt64() -> BinaryenOp;
    pub fn BinaryenSubInt64() -> BinaryenOp;
    pub fn BinaryenMulInt64() -> BinaryenOp;
    pub fn BinaryenDivSInt64() -> BinaryenOp;
    pub fn BinaryenDivUInt64() -> BinaryenOp;
    pub fn BinaryenRemSInt64() -> BinaryenOp;
    pub fn BinaryenRemUInt64() -> BinaryenOp;
    pub fn BinaryenAndInt64() -> BinaryenOp;
    pub fn BinaryenOrInt64() -> BinaryenOp;
    pub fn BinaryenXorInt64() -> BinaryenOp;
    pub fn BinaryenShlInt64() -> BinaryenOp;
    pub fn BinaryenShrUInt64() -> BinaryenOp;
    pub fn BinaryenShrSInt64() -> BinaryenOp;
    pub fn BinaryenRotLInt64() -> BinaryenOp;
    pub fn BinaryenRotRInt64() -> BinaryenOp;
    pub fn BinaryenEqInt64() -> BinaryenOp;
    pub fn BinaryenNeInt64() -> BinaryenOp;
    pub fn BinaryenLtSInt64() -> BinaryenOp;
    pub fn BinaryenLtUInt64() -> BinaryenOp;
    pub fn BinaryenLeSInt64() -> BinaryenOp;
    pub fn BinaryenLeUInt64() -> BinaryenOp;
    pub fn BinaryenGtSInt64() -> BinaryenOp;
    pub fn BinaryenGtUInt64() -> BinaryenOp;
    pub fn BinaryenGeSInt64() -> BinaryenOp;
    pub fn BinaryenGeUInt64() -> BinaryenOp;
    pub fn BinaryenAddFloat32() -> BinaryenOp;
    pub fn BinaryenSubFloat32() -> BinaryenOp;
    pub fn BinaryenMulFloat32() -> BinaryenOp;
    pub fn BinaryenDivFloat32() -> BinaryenOp;
    pub fn BinaryenCopySignFloat32() -> BinaryenOp;
    pub fn BinaryenMinFloat32() -> BinaryenOp;
    pub fn BinaryenMaxFloat32() -> BinaryenOp;
    pub fn BinaryenEqFloat32() -> BinaryenOp;
    pub fn BinaryenNeFloat32() -> BinaryenOp;
    pub fn BinaryenLtFloat32() -> BinaryenOp;
    pub fn BinaryenLeFloat32() -> BinaryenOp;
    pub fn BinaryenGtFloat32() -> BinaryenOp;
    pub fn BinaryenGeFloat32() -> BinaryenOp;
    pub fn BinaryenAddFloat64() -> BinaryenOp;
    pub fn BinaryenSubFloat64() -> BinaryenOp;
    pub fn BinaryenMulFloat64() -> BinaryenOp;
    pub fn BinaryenDivFloat64() -> BinaryenOp;
    pub fn BinaryenCopySignFloat64() -> BinaryenOp;
    pub fn BinaryenMinFloat64() -> BinaryenOp;
    pub fn BinaryenMaxFloat64() -> BinaryenOp;
    pub fn BinaryenEqFloat64() -> BinaryenOp;
    pub fn BinaryenNeFloat64() -> BinaryenOp;
    pub fn BinaryenLtFloat64() -> BinaryenOp;
    pub fn BinaryenLeFloat64() -> BinaryenOp;
    pub fn BinaryenGtFloat64() -> BinaryenOp;
    pub fn BinaryenGeFloat64() -> BinaryenOp;
    pub fn BinaryenPageSize() -> BinaryenOp;
    pub fn BinaryenCurrentMemory() -> BinaryenOp;
    pub fn BinaryenGrowMemory() -> BinaryenOp;
    pub fn BinaryenHasFeature() -> BinaryenOp;
}

extern "C" {

    // Modules
    pub fn BinaryenModuleCreate() -> BinaryenModuleRef;
    pub fn BinaryenModuleDispose(module: BinaryenModuleRef);

    // Function types

    pub fn BinaryenAddFunctionType(module: BinaryenModuleRef,
                                   name: *const c_char,
                                   result: BinaryenType,
                                   paramTypes: *const BinaryenType,
                                   numParams: BinaryenIndex)
                                   -> BinaryenFunctionTypeRef;

    // Expressions

    pub fn BinaryenBlock(module: BinaryenModuleRef,
                         name: *const c_char,
                         children: *const BinaryenExpressionRef,
                         numChildren: BinaryenIndex)
                         -> BinaryenExpressionRef;
    pub fn BinaryenIf(module: BinaryenModuleRef,
                      condition: BinaryenExpressionRef,
                      ifTrue: BinaryenExpressionRef,
                      ifFalse: BinaryenExpressionRef)
                      -> BinaryenExpressionRef;
    pub fn BinaryenLoop(module: BinaryenModuleRef,
                        in_: *const c_char,
                        body: BinaryenExpressionRef)
                        -> BinaryenExpressionRef;
    pub fn BinaryenBreak(module: BinaryenModuleRef,
                         name: *const c_char,
                         condition: BinaryenExpressionRef,
                         value: BinaryenExpressionRef)
                         -> BinaryenExpressionRef;
    pub fn BinaryenSwitch(module: BinaryenModuleRef,
                          names: *const *const c_char,
                          numNames: BinaryenIndex,
                          defaultName: *const c_char,
                          condition: BinaryenExpressionRef,
                          value: BinaryenExpressionRef)
                          -> BinaryenExpressionRef;
    pub fn BinaryenCall(module: BinaryenModuleRef,
                        target: *const c_char,
                        operands: *const BinaryenExpressionRef,
                        numOperands: BinaryenIndex,
                        returnType: BinaryenType)
                        -> BinaryenExpressionRef;
    pub fn BinaryenCallImport(module: BinaryenModuleRef,
                              target: *const c_char,
                              operands: *const BinaryenExpressionRef,
                              numOperands: BinaryenIndex,
                              returnType: BinaryenType)
                              -> BinaryenExpressionRef;
    pub fn BinaryenCallIndirect(module: BinaryenModuleRef,
                                target: BinaryenExpressionRef,
                                operands: *const BinaryenExpressionRef,
                                numOperands: BinaryenIndex,
                                type_: BinaryenFunctionTypeRef)
                                -> BinaryenExpressionRef;
    pub fn BinaryenGetLocal(module: BinaryenModuleRef,
                            index: BinaryenIndex,
                            type_: BinaryenType)
                            -> BinaryenExpressionRef;
    pub fn BinaryenSetLocal(module: BinaryenModuleRef,
                            index: BinaryenIndex,
                            value: BinaryenExpressionRef)
                            -> BinaryenExpressionRef;
    pub fn BinaryenTeeLocal(module: BinaryenModuleRef,
                            index: BinaryenIndex,
                            value: BinaryenExpressionRef)
                            -> BinaryenExpressionRef;
    pub fn BinaryenLoad(module: BinaryenModuleRef,
                        bytes: u32,
                        signed_: u8,
                        offset: u32,
                        align: u32,
                        type_: BinaryenType,
                        ptr: BinaryenExpressionRef)
                        -> BinaryenExpressionRef;
    pub fn BinaryenStore(module: BinaryenModuleRef,
                         bytes: u32,
                         offset: u32,
                         align: u32,
                         ptr: BinaryenExpressionRef,
                         value: BinaryenExpressionRef,
                         _type: BinaryenType)
                         -> BinaryenExpressionRef;
    pub fn BinaryenConst(module: BinaryenModuleRef,
                         value: BinaryenLiteral)
                         -> BinaryenExpressionRef;
    pub fn BinaryenUnary(module: BinaryenModuleRef,
                         op: BinaryenOp,
                         value: BinaryenExpressionRef)
                         -> BinaryenExpressionRef;
    pub fn BinaryenBinary(module: BinaryenModuleRef,
                          op: BinaryenOp,
                          left: BinaryenExpressionRef,
                          right: BinaryenExpressionRef)
                          -> BinaryenExpressionRef;
    pub fn BinaryenSelect(module: BinaryenModuleRef,
                          condition: BinaryenExpressionRef,
                          ifTrue: BinaryenExpressionRef,
                          ifFalse: BinaryenExpressionRef)
                          -> BinaryenExpressionRef;
    pub fn BinaryenDrop(module: BinaryenModuleRef,
                        value: BinaryenExpressionRef)
                        -> BinaryenExpressionRef;
    pub fn BinaryenReturn(module: BinaryenModuleRef,
                          value: BinaryenExpressionRef)
                          -> BinaryenExpressionRef;
    pub fn BinaryenHost(module: BinaryenModuleRef,
                        op: BinaryenOp,
                        name: *const c_char,
                        operands: *const BinaryenExpressionRef,
                        numOperands: BinaryenIndex)
                        -> BinaryenExpressionRef;
    pub fn BinaryenNop(module: BinaryenModuleRef) -> BinaryenExpressionRef;
    pub fn BinaryenUnreachable(module: BinaryenModuleRef) -> BinaryenExpressionRef;
    pub fn BinaryenExpressionPrint(expr: BinaryenExpressionRef);

    // Functions

    pub fn BinaryenAddFunction(module: BinaryenModuleRef,
                               name: *const c_char,
                               type_: BinaryenFunctionTypeRef,
                               localTypes: *const BinaryenType,
                               numLocalTypes: BinaryenIndex,
                               body: BinaryenExpressionRef)
                               -> BinaryenFunctionRef;

    // Imports

    pub fn BinaryenAddImport(module: BinaryenModuleRef,
                             internalName: *const c_char,
                             externalModuleName: *const c_char,
                             externalBaseName: *const c_char,
                             type_: BinaryenFunctionTypeRef)
                             -> BinaryenImportRef;

    // Exports

    pub fn BinaryenAddExport(module: BinaryenModuleRef,
                             internalName: *const c_char,
                             externalName: *const c_char)
                             -> BinaryenExportRef;

    // Function table. One per module

    pub fn BinaryenSetFunctionTable(module: BinaryenModuleRef,
                                    functions: *const BinaryenFunctionRef,
                                    numFunctions: BinaryenIndex);

    // Memory

    pub fn BinaryenSetMemory(module: BinaryenModuleRef,
                             initial: BinaryenIndex,
                             maximum: BinaryenIndex,
                             exportName: *const c_char,
                             segments: *const *const c_char,
                             segmentOffsets: *const BinaryenExpressionRef,
                             segmentSizes: *const BinaryenIndex,
                             numSegments: BinaryenIndex);

    // Start function

    pub fn BinaryenSetStart(module: BinaryenModuleRef, start: BinaryenFunctionRef);

    // Module Operations

    pub fn BinaryenModulePrint(module: BinaryenModuleRef);

    pub fn BinaryenModuleValidate(module: BinaryenModuleRef) -> i32;

    pub fn BinaryenModuleOptimize(module: BinaryenModuleRef);

    pub fn BinaryenModuleWrite(module: BinaryenModuleRef,
                               output: *mut c_char,
                               outputSize: size_t)
                               -> size_t;

    pub fn BinaryenModuleRead(input: *const c_char, inputSize: size_t) -> BinaryenModuleRef;

    pub fn BinaryenModuleInterpret(module: BinaryenModuleRef);

    pub fn BinaryenModuleAutoDrop(module: BinaryenModuleRef);

    // CFG / Relooper

    pub fn RelooperCreate() -> RelooperRef;

    pub fn RelooperAddBlock(relooper: RelooperRef,
                            code: BinaryenExpressionRef)
                            -> RelooperBlockRef;

    pub fn RelooperAddBranch(from: RelooperBlockRef,
                             to: RelooperBlockRef,
                             condition: BinaryenExpressionRef,
                             code: BinaryenExpressionRef);

    pub fn RelooperAddBlockWithSwitch(relooper: RelooperRef,
                                      code: BinaryenExpressionRef,
                                      condition: BinaryenExpressionRef)
                                      -> RelooperBlockRef;

    pub fn RelooperAddBranchForSwitch(from: RelooperBlockRef,
                                      to: RelooperBlockRef,
                                      indexes: *const BinaryenIndex,
                                      numIndexes: BinaryenIndex,
                                      code: BinaryenExpressionRef);

    pub fn RelooperRenderAndDispose(relooper: RelooperRef,
                                    entry: RelooperBlockRef,
                                    labelHelper: BinaryenIndex,
                                    module: BinaryenModuleRef)
                                    -> BinaryenExpressionRef;

    // Other APIs

    pub fn BinaryenSetAPITracing(on: bool);
}
