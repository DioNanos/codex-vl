//! Word-based chunking with optional overlap. Default `chunk_size = 64` words
//! follows the MSA paper pooling kernel `P = 64`. Tokens are word-level here,
//! not BPE — the routing surrogate is BM25, so tokenizer alignment with the
//! generator's BPE is not required.

use unicode_segmentation::UnicodeSegmentation;

use crate::schema::ChunkConfig;

#[derive(Debug, Clone)]
pub struct TextChunk {
    pub text: String,
    pub token_count: usize,
    /// `(start, end)` in the *original document text*, character offsets.
    pub char_offset: (usize, usize),
}

/// Split `text` into chunks of `cfg.chunk_size` words with `cfg.overlap`
/// words of overlap between adjacent chunks. Returns an empty vec for empty
/// input.
pub fn chunk_text(text: &str, cfg: &ChunkConfig) -> Vec<TextChunk> {
    let words: Vec<(usize, &str)> = text.unicode_word_indices().collect();
    if words.is_empty() {
        return vec![];
    }

    let chunk_size = cfg.chunk_size.max(1);
    let overlap = cfg.overlap.min(chunk_size.saturating_sub(1));
    let step = chunk_size.saturating_sub(overlap).max(1);

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < words.len() {
        let end = (start + chunk_size).min(words.len());
        let char_start = words[start].0;
        let char_end = {
            let (last_offset, last_word) = words[end - 1];
            last_offset + last_word.len()
        };
        let slice = &text[char_start..char_end];
        chunks.push(TextChunk {
            text: slice.to_string(),
            token_count: end - start,
            char_offset: (char_start, char_end),
        });
        if end >= words.len() {
            break;
        }
        start += step;
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_returns_empty() {
        assert!(chunk_text("", &ChunkConfig::default()).is_empty());
        assert!(chunk_text("   ", &ChunkConfig::default()).is_empty());
    }

    #[test]
    fn basic_split_no_overlap() {
        let cfg = ChunkConfig {
            chunk_size: 3,
            overlap: 0,
        };
        let chunks = chunk_text("uno due tre quattro cinque sei sette", &cfg);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "uno due tre");
        assert_eq!(chunks[1].text, "quattro cinque sei");
        assert_eq!(chunks[2].text, "sette");
    }

    #[test]
    fn overlap_preserves_continuity() {
        let cfg = ChunkConfig {
            chunk_size: 4,
            overlap: 2,
        };
        let chunks = chunk_text("A B C D E F G H", &cfg);
        // step = 2 → [A B C D], [C D E F], [E F G H]
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].text, "A B C D");
        assert_eq!(chunks[1].text, "C D E F");
        assert_eq!(chunks[2].text, "E F G H");
    }

    #[test]
    fn char_offsets_are_within_input() {
        let text = "alpha bravo charlie delta";
        let cfg = ChunkConfig {
            chunk_size: 2,
            overlap: 0,
        };
        let chunks = chunk_text(text, &cfg);
        for c in &chunks {
            let slice = &text[c.char_offset.0..c.char_offset.1];
            assert_eq!(slice, c.text);
        }
    }
}
