//! Typed conversion of Grobid/Pub2TEI documents into the pipeline's working model.
//!
//! TEI deliberately permits many equivalent encodings. The private parser
//! accepts that flexibility at the XML boundary while this module exposes a
//! small, strongly typed API to the rest of the application.

mod parser;
mod service;

pub use crate::models::draft::*;
pub use service::{TeiConversionService, TeiValidationWarning};
