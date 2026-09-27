//! Editing an existing text buffer without interpreting its contents.
pub(crate) struct TextInput<'a> {
    text: &'a mut String,
}
impl<'a> TextInput<'a> {
    pub fn new(text: &'a mut String) -> Self {
        Self { text }
    }
    pub fn edit(self, c: Option<char>) {
        match c {
            Some(c) => self.text.push(c),
            None => {
                self.text.pop();
            }
        }
    }
}
