mod format;
mod header;
mod reader;
mod trim;
mod writer;

pub use format::{SampleKind, WavError, WavFormat};
pub use header::{ParsedHeader, build_header, header_len, parse_header};
pub use reader::{WavReader, repair, repaired_header};
pub use trim::{part_path, reverse_frames, write_range, write_trimmed, write_variant};
pub use writer::WavWriter;
