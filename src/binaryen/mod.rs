pub mod builder;
pub mod sys;

pub use self::sys::*;

pub fn set_api_tracing(trace: bool) {
    unsafe { BinaryenSetAPITracing(true) }
}
