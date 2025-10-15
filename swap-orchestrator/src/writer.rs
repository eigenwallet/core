use std::fmt::Write;

/// Helper struct to make writing to a docker-compose.yml easier.
/// Implements [`std::fmt::Write`], so you can use the `write!` macro on it.
/// Keeps track of the current indentation level and
pub struct IndentedWriter {
    /// Inner string buffer we write to under the hood.
    buffer: String,
    /// Current indentation level - we multiply this by two to get the number of spaces.
    current_indentation: usize,
}

impl IndentedWriter {
    const SPACES_PER_INDENTATION: usize = 2;
    const WHITESPACE: char = ' ';

    /// Start with a new, empty string and zero indentation.
    pub fn new() -> IndentedWriter {
        IndentedWriter {
            buffer: String::new(),
            current_indentation: 0,
        }
    }

    /// Finish writing and return the final String buffer.
    pub fn finish(self) -> String {
        self.buffer
    }

    /// Get scoped access to the writer but with one more level of indentation.
    ///
    /// # Example
    ///
    /// ```
    /// use swap_orchestrator::writer::IndentedWriter;
    /// use std::fmt::Write;
    ///
    /// let mut writer = IndentedWriter::new();
    /// writeln!(&mut writer, "version: 3");
    /// writeln!(&mut writer, "services:");
    ///
    /// writer.indented(|writer| {
    ///     writeln!(writer, "monerod:");
    ///     writer.indented(|writer| {
    ///         writeln!(writer, "container_name: monerod");
    ///     });
    /// });
    ///
    /// assert_eq!(
    ///     &writer.finish(),
    ///     "version: 3
    /// services:
    ///   monerod:
    ///     container_name: monerod
    /// "
    /// )
    /// ```
    pub fn indented<T>(&mut self, closure: impl FnOnce(&mut IndentedWriter) -> T) -> T {
        self.current_indentation += 1;
        let result = closure(self);
        // No underflow possible because we just increased the number and don't change
        // it anywhere else
        self.current_indentation -= 1;

        result
    }
}

impl Write for IndentedWriter {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        let is_new_line = match self.buffer.chars().last() {
            Some('\n') => true,
            Some(_) => false,
            None => true,
        };
        let indentation = Self::WHITESPACE
            .to_string()
            .repeat(Self::SPACES_PER_INDENTATION * self.current_indentation * is_new_line as usize);

        write!(&mut self.buffer, "{indentation}{value}")
    }
}
