use rowan::TextSize;

/// Maps byte offsets to 1-based line and column numbers.
#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<u32>,
}

impl LineIndex {
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        line_starts.extend(
            text.bytes()
                .enumerate()
                .filter(|(_, b)| *b == b'\n')
                .map(|(i, _)| i as u32 + 1),
        );
        Self { line_starts }
    }

    /// 1-based (line, column); the column counts bytes.
    pub fn line_col(&self, offset: TextSize) -> (u32, u32) {
        let offset = u32::from(offset);
        let line = self.line_starts.partition_point(|&start| start <= offset) - 1;
        (line as u32 + 1, offset - self.line_starts[line] + 1)
    }

    pub fn line(&self, offset: TextSize) -> u32 {
        self.line_col(offset).0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_columns() {
        let index = LineIndex::new("ab\ncd\n\nx");
        assert_eq!(index.line_col(0.into()), (1, 1));
        assert_eq!(index.line_col(1.into()), (1, 2));
        assert_eq!(index.line_col(3.into()), (2, 1));
        assert_eq!(index.line_col(6.into()), (3, 1));
        assert_eq!(index.line_col(7.into()), (4, 1));
    }
}
