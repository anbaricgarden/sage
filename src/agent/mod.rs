pub mod editor;

pub trait Agent {
    fn name(&self) -> &'static str;
}
