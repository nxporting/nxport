// Instruction emitters (Ryujinx.Graphics.Shader.Instructions): one function per Maxwell instruction form.

mod attribute;
mod bits;
mod conversion;
mod float;
mod float_compare;
mod flow;
pub mod helper;
mod int_compare;
mod integer;
mod memory;
mod moves;
mod surface;
mod texture;
mod unimplemented;
mod warp_video;

pub use attribute::*;
pub use bits::*;
pub use conversion::*;
pub use float::*;
pub use float_compare::*;
pub use flow::*;
pub use int_compare::*;
pub use integer::*;
pub use memory::*;
pub use moves::*;
pub use surface::*;
pub use texture::*;
pub use unimplemented::*;
pub use warp_video::*;

pub type InstEmitter = fn(&mut crate::translation::emitter::EmitterContext);
