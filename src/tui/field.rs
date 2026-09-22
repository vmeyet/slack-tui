//! The one-row input: its text and where the cursor sits, in one value so the two cannot drift.
//! The cursor is a byte offset kept on a character boundary, so an accent or an emoji never splits.

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Field {
    text: String,
    cursor: usize,
}

impl Field {
    /// Starts on `text` with the cursor after its last character, where typing goes on.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self { cursor: text.len(), text }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Hands the text over and leaves the row empty.
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }

    pub fn at_end(&self) -> bool {
        self.cursor == self.text.len()
    }

    /// What sits before the cursor, the character under it (empty past the last one), and the rest.
    pub fn split(&self) -> (&str, &str, &str) {
        let under = self.next().map_or("", |end| &self.text[self.cursor..end]);
        (&self.text[..self.cursor], under, &self.text[self.cursor + under.len()..])
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    pub fn backspace(&mut self) {
        let Some(prev) = self.prev() else { return };
        self.text.remove(prev);
        self.cursor = prev;
    }

    pub fn delete(&mut self) {
        if !self.at_end() {
            self.text.remove(self.cursor);
        }
    }

    /// Deletes back to the start of the word the cursor follows, the spaces in between included.
    pub fn delete_word(&mut self) {
        let from = self.word_start();
        self.text.replace_range(from..self.cursor, "");
        self.cursor = from;
    }

    pub fn left(&mut self) {
        self.cursor = self.prev().unwrap_or(0);
    }

    pub fn right(&mut self) {
        self.cursor = self.next().unwrap_or(self.cursor);
    }

    pub fn start(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.text.len();
    }

    fn prev(&self) -> Option<usize> {
        self.text[..self.cursor].char_indices().next_back().map(|(i, _)| i)
    }

    fn next(&self) -> Option<usize> {
        self.text[self.cursor..].chars().next().map(|c| self.cursor + c.len_utf8())
    }

    fn word_start(&self) -> usize {
        let word = self.text[..self.cursor].trim_end();
        match word.char_indices().rev().find(|(_, c)| c.is_whitespace()) {
            Some((i, space)) => i + space.len_utf8(),
            None => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    /// The text with the cursor drawn in it, so a test reads as what the row shows.
    fn shown(field: &Field) -> String {
        let (before, under, after) = field.split();
        format!("{before}|{under}{after}")
    }

    fn typed(text: &str) -> Field {
        let mut field = Field::default();
        for c in text.chars() {
            field.insert(c);
        }
        field
    }

    #[test]
    fn typing_leaves_the_cursor_after_the_last_character() {
        assert_eq!(shown(&typed("hey")), "hey|");
        assert_eq!(typed("hey").text(), "hey");
    }

    #[test]
    fn prefilled_text_starts_with_the_cursor_at_the_end() {
        assert_eq!(shown(&Field::new("hey")), "hey|");
        assert_eq!(shown(&Field::default()), "|");
    }

    #[test]
    fn insert_lands_where_the_cursor_is() {
        let mut field = Field::new("hey");
        field.left();
        field.insert('l');
        assert_eq!(shown(&field), "hel|y");
    }

    #[test]
    fn movement_stops_at_both_ends() {
        let mut field = Field::new("ab");
        field.right();
        assert_eq!(shown(&field), "ab|");
        field.left();
        field.left();
        field.left();
        assert_eq!(shown(&field), "|ab");
    }

    #[test]
    fn home_and_end_jump_to_the_edges() {
        let mut field = Field::new("hey");
        field.start();
        assert_eq!(shown(&field), "|hey");
        field.end();
        assert_eq!(shown(&field), "hey|");
    }

    #[test]
    fn backspace_eats_the_character_before_the_cursor_and_nothing_at_the_start() {
        let mut field = Field::new("hey");
        field.backspace();
        assert_eq!(shown(&field), "he|");
        field.start();
        field.backspace();
        assert_eq!(shown(&field), "|he");
        field.right();
        field.backspace();
        assert_eq!(shown(&field), "|e");
    }

    #[test]
    fn delete_eats_the_character_under_the_cursor_and_nothing_at_the_end() {
        let mut field = Field::new("hey");
        field.delete();
        assert_eq!(shown(&field), "hey|");
        field.start();
        field.delete();
        assert_eq!(shown(&field), "|ey");
        field.right();
        field.delete();
        assert_eq!(shown(&field), "e|");
    }

    #[test]
    fn delete_word_takes_the_word_before_the_cursor_with_its_trailing_spaces() {
        let mut field = Field::new("ship it now");
        field.delete_word();
        assert_eq!(shown(&field), "ship it |");

        let mut spaced = Field::new("ship it   ");
        spaced.delete_word();
        assert_eq!(shown(&spaced), "ship |");

        let mut only_word = Field::new("ship");
        only_word.delete_word();
        assert_eq!(shown(&only_word), "|");

        let mut empty = Field::default();
        empty.delete_word();
        assert_eq!(shown(&empty), "|");
    }

    #[test]
    fn delete_word_keeps_what_follows_the_cursor() {
        let mut field = Field::new("ship it now");
        field.start();
        field.right();
        field.right();
        field.right();
        field.right();
        field.delete_word();
        assert_eq!(shown(&field), "| it now");
    }

    #[test]
    fn accents_move_and_delete_as_one_character() {
        let mut field = Field::new("café");
        field.left();
        assert_eq!(shown(&field), "caf|é");
        field.insert('è');
        assert_eq!(shown(&field), "cafè|é");
        field.backspace();
        assert_eq!(shown(&field), "caf|é");
        field.delete();
        assert_eq!(shown(&field), "caf|");
    }

    #[test]
    fn emoji_move_and_delete_as_one_character() {
        let mut field = Field::new("a🚀b");
        field.start();
        field.right();
        assert_eq!(shown(&field), "a|🚀b");
        field.right();
        assert_eq!(shown(&field), "a🚀|b");
        field.backspace();
        assert_eq!(shown(&field), "a|b");
        field.insert('🎉');
        assert_eq!(shown(&field), "a🎉|b");
        field.delete();
        assert_eq!(shown(&field), "a🎉|");
    }

    #[test]
    fn delete_word_splits_multi_byte_words_on_their_spaces() {
        let mut field = Field::new("café 🚀 fusée");
        field.delete_word();
        assert_eq!(shown(&field), "café 🚀 |");
        field.delete_word();
        assert_eq!(shown(&field), "café |");
    }

    #[test]
    fn take_hands_the_text_over_and_resets_the_row() {
        let mut field = Field::new("café");
        assert_eq!(field.take(), "café");
        assert_eq!(shown(&field), "|");
        field.insert('a');
        assert_eq!(shown(&field), "a|");
        field.clear();
        assert_eq!(shown(&field), "|");
    }
}
