use crate::{FFIResult, Ptr, Rid, WasmEnv};
use wasmer::FunctionEnvMut;

const UNSUPPORTED: FFIResult = -1;

pub fn new_context(_env: FunctionEnvMut<WasmEnv>, _width: f32, _height: f32) -> Rid {
    UNSUPPORTED
}
pub fn set_transform(
    _env: FunctionEnvMut<WasmEnv>,
    _context: Rid,
    _tx: f32,
    _ty: f32,
    _sx: f32,
    _sy: f32,
    _angle: f32,
) -> FFIResult {
    UNSUPPORTED
}
pub fn draw_image(
    _env: FunctionEnvMut<WasmEnv>,
    _context: Rid,
    _image: Rid,
    _x: f32,
    _y: f32,
    _width: f32,
    _height: f32,
) -> FFIResult {
    UNSUPPORTED
}
pub fn copy_image(
    _env: FunctionEnvMut<WasmEnv>,
    _context: Rid,
    _image: Rid,
    _sx: f32,
    _sy: f32,
    _sw: f32,
    _sh: f32,
    _dx: f32,
    _dy: f32,
    _dw: f32,
    _dh: f32,
) -> FFIResult {
    UNSUPPORTED
}
pub fn fill(
    _env: FunctionEnvMut<WasmEnv>,
    _context: Rid,
    _path: Ptr,
    _red: f32,
    _green: f32,
    _blue: f32,
    _alpha: f32,
) -> FFIResult {
    UNSUPPORTED
}
pub fn stroke(_env: FunctionEnvMut<WasmEnv>, _context: Rid, _path: Ptr, _style: Ptr) -> FFIResult {
    UNSUPPORTED
}
pub fn draw_text(
    _env: FunctionEnvMut<WasmEnv>,
    _context: Rid,
    _text: Ptr,
    _text_len: u32,
    _size: f32,
    _x: f32,
    _y: f32,
    _font: Rid,
    _red: f32,
    _green: f32,
    _blue: f32,
    _alpha: f32,
) -> FFIResult {
    UNSUPPORTED
}
pub fn get_image(_env: FunctionEnvMut<WasmEnv>, _context: Rid) -> Rid {
    UNSUPPORTED
}
pub fn new_font(_env: FunctionEnvMut<WasmEnv>, _name: Ptr, _name_len: u32) -> FFIResult {
    UNSUPPORTED
}
pub fn system_font(_env: FunctionEnvMut<WasmEnv>, _weight: u8) -> Rid {
    UNSUPPORTED
}
pub fn load_font(_env: FunctionEnvMut<WasmEnv>, _url: Ptr, _url_len: u32) -> FFIResult {
    UNSUPPORTED
}
pub fn new_image(_env: FunctionEnvMut<WasmEnv>, _data: Ptr, _data_len: u32) -> FFIResult {
    UNSUPPORTED
}
pub fn get_image_data(_env: FunctionEnvMut<WasmEnv>, _image: Rid) -> FFIResult {
    UNSUPPORTED
}
pub fn get_image_width(_env: FunctionEnvMut<WasmEnv>, _image: Rid) -> f32 {
    UNSUPPORTED as f32
}
pub fn get_image_height(_env: FunctionEnvMut<WasmEnv>, _image: Rid) -> f32 {
    UNSUPPORTED as f32
}
