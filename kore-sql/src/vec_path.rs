use kore_core::{DataBlock, KoreError};
use crate::ast::Query;
use crate::executor::KqlContext;

pub fn try_vectorized(_query: &Query, _ctx: &KqlContext) -> Option<Result<DataBlock, KoreError>> {
    None
}
