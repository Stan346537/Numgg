use numbat::markup::{FormatType, FormattedString, Formatter, Markup};

use colored::Colorize;

pub struct ANSIFormatter;

impl Formatter for ANSIFormatter {
    fn format_part(
        &self,
        FormattedString(_output_type, format_type, text): &FormattedString,
    ) -> String {
        (match format_type {
            FormatType::Whitespace => text.normal(),
            FormatType::Keyword => text.magenta(),
            FormatType::Value => text.yellow(),
            FormatType::Unit => text.cyan(),
            FormatType::Identifier => text.normal(),
            FormatType::TypeIdentifier => text.blue().italic(),
            FormatType::Operator => text.bold(),
            FormatType::Decorator => text.green(),
        })
        .to_string()
    }
}

pub fn ansi_format(m: &Markup, indent: bool) -> String {
    ANSIFormatter {}.format(m, indent)
}
