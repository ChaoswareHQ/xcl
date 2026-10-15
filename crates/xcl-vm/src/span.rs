#[derive(Debug, Clone)]
pub struct Span {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

impl Span {
    #[inline]
    pub fn new(file: String, line: usize, col: usize) -> Self {
        Self { file, line, col }
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            file: String::new(),
            line: 1,
            col: 1,
        }
    }
}
