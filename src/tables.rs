//! Markdown table edits retain source outside the requested cell or table.
use pulldown_cmark::Alignment;
use std::ops::Range;

#[derive(Clone, Debug)]
pub struct Row {
    pub cells: Vec<Range<usize>>,
    fields: Vec<Range<usize>>,
    line: Range<usize>,
    prefix: String,
    ending: String,
}

#[derive(Clone, Debug)]
pub struct Table {
    pub range: Range<usize>,
    pub alignments: Vec<Alignment>,
    pub rows: Vec<Row>,
    snapshot: String,
    separator: Row,
}

/// Retain typed padding until the field closes; Markdown trims it when reparsed.
#[derive(Clone, Debug)]
pub struct CellDraft {
    pub text: String,
    row: usize,
    column: usize,
    range: Range<usize>,
    encoded: String,
}

impl CellDraft {
    pub fn is_current(&self, source: &str) -> bool {
        source.get(self.range.clone()) == Some(self.encoded.as_str())
    }
}

#[derive(Clone, Debug)]
pub enum Action {
    Cell {
        row: usize,
        column: usize,
        text: String,
    },
    InsertRow(usize),
    RemoveRow(usize),
    MoveRow {
        from: usize,
        to: usize,
    },
    InsertColumn(usize),
    RemoveColumn(usize),
    MoveColumn {
        from: usize,
        to: usize,
    },
    Align {
        column: usize,
        alignment: Alignment,
    },
    Sort {
        column: usize,
        descending: bool,
    },
    /// Empty every cell in a rectangular selection; the header row may be included.
    Clear {
        rows: Range<usize>,
        columns: Range<usize>,
    },
    /// Body rows only: the header row stays.
    RemoveRows(Range<usize>),
    /// At least one column must remain.
    RemoveColumns(Range<usize>),
    /// Paste a block of cells (a spreadsheet range) with its top left corner on `row`, `column`.
    /// The table grows with body rows and columns when the block does not fit; the text of each
    /// cell is plain (pipes are escaped here) and replaces what was there.
    Fill {
        row: usize,
        column: usize,
        cells: Vec<Vec<String>>,
    },
}

impl Table {
    pub fn is_current(&self, source: &str) -> bool {
        source.get(self.range.clone()) == Some(self.snapshot.as_str())
    }
    /// Ranges come from the existing Markdown parser, never a pipe-line heuristic.
    pub fn from_ranges(
        source: &str,
        range: Range<usize>,
        alignments: Vec<Alignment>,
        row_ranges: Vec<Range<usize>>,
    ) -> Option<Self> {
        if alignments.is_empty() || row_ranges.is_empty() {
            return None;
        }
        let rows: Vec<_> = row_ranges
            .into_iter()
            .map(|range| Row::parse(source, range.start))
            .collect::<Option<_>>()?;
        let separator_start = rows[0].line.end;
        let separator_end = source[separator_start..]
            .find('\n')
            .map_or(source.len(), |end| separator_start + end + 1);
        if separator_start >= range.end || separator_end > range.end {
            return None;
        }
        let marker = source[separator_start..separator_end].find(['|', '-', ':'])?;
        let separator = Row::parse(source, separator_start + marker)?;
        if separator.cells.len() != alignments.len() {
            return None;
        }
        let range = rows[0].line.start..rows.last()?.line.end.max(separator_end);
        Some(Self {
            snapshot: source.get(range.clone())?.into(),
            range,
            alignments,
            rows,
            separator,
        })
    }

    /// The cell a source position belongs to; the separator line counts as the header.
    pub fn cell_at(&self, byte: usize) -> (usize, usize) {
        let row = self
            .rows
            .iter()
            .rposition(|row| row.line.start <= byte)
            .unwrap_or(0);
        let cells = if row == 0 && byte >= self.rows[0].line.end {
            &self.separator.cells
        } else {
            &self.rows[row].cells
        };
        let column = cells
            .iter()
            .rposition(|cell| cell.start <= byte)
            .unwrap_or(0)
            .min(self.alignments.len() - 1);
        (row, column)
    }

    pub fn cell<'a>(&self, source: &'a str, row: usize, column: usize) -> &'a str {
        self.rows
            .get(row)
            .and_then(|row| row.cells.get(column))
            .and_then(|range| source.get(range.clone()))
            .unwrap_or("")
    }

    /// A field edits literal pipes; only the table's delimiter escape is decoded.
    pub fn cell_text(&self, source: &str, row: usize, column: usize) -> String {
        decode_pipes(self.cell(source, row, column))
    }

    pub fn edit_cell(
        &self,
        source: &mut String,
        row: usize,
        column: usize,
        text: String,
        draft: Option<&CellDraft>,
    ) -> Result<CellDraft, &'static str> {
        if !self.is_current(source) {
            return Err("A tabela mudou. Reabra a célula antes de editá-la.");
        }
        if column >= self.alignments.len() || row >= self.rows.len() || text.contains(['\n', '\r'])
        {
            return Err(
                "Uma célula de tabela deve ser uma linha Markdown em uma coluna existente.",
            );
        }
        let encoded = encode_pipes(&text);
        let target = &self.rows[row];
        let tracked = draft.filter(|draft| {
            draft.row == row
                && draft.column == column
                && draft.is_current(source)
                && target.fields.get(column).is_some_and(|field| {
                    field.start <= draft.range.start && field.end >= draft.range.end
                })
        });
        let start = if let Some(range) = tracked
            .map(|draft| &draft.range)
            .or_else(|| target.cells.get(column))
        {
            let start = range.start;
            source.replace_range(range.clone(), &encoded);
            start
        } else {
            // GFM permits short rows. Materialize only this row's missing cells.
            let mut replacement = format!("{}|", target.prefix);
            let mut start = 0;
            for index in 0..self.alignments.len() {
                replacement.push(' ');
                if index == column {
                    start = target.line.start + replacement.len();
                    replacement.push_str(&encoded);
                } else {
                    replacement.push_str(self.cell(source, row, index));
                }
                replacement.push_str(" |");
            }
            replacement.push_str(&target.ending);
            source.replace_range(target.line.clone(), &replacement);
            start
        };
        Ok(CellDraft {
            row,
            column,
            range: start..start + encoded.len(),
            text,
            encoded,
        })
    }

    /// The selected cells as a standalone Markdown table; its first row becomes the header.
    pub fn copy(&self, source: &str, rows: Range<usize>, columns: Range<usize>) -> String {
        let mut output = String::new();
        for (index, row) in rows.clone().enumerate() {
            write_row(
                &mut output,
                "",
                columns.clone().map(|column| self.cell(source, row, column)),
            );
            output.push('\n');
            if index == 0 {
                write_row(&mut output, "", columns.clone().map(|_| "---"));
                output.push('\n');
            }
        }
        output
    }

    pub fn apply(&self, source: &mut String, action: Action) -> Result<(), &'static str> {
        if let Action::Cell { row, column, text } = action {
            return self.edit_cell(source, row, column, text, None).map(|_| ());
        }
        if !self.is_current(source) {
            return Err("A tabela mudou. Reabra a célula antes de editá-la.");
        }
        let columns = self.alignments.len();
        let mut rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                (0..row.cells.len().max(columns))
                    .map(|column| self.cell(source, index, column).to_owned())
                    .collect()
            })
            .collect();
        let mut alignments = self.alignments.clone();
        match action {
            Action::InsertRow(index) if index > 0 && index <= rows.len() => {
                rows.insert(index, vec![String::new(); columns]);
            }
            Action::RemoveRow(index) if index > 0 && index < rows.len() => {
                rows.remove(index);
            }
            Action::MoveRow { from, to }
                if from > 0 && to > 0 && from < rows.len() && to < rows.len() =>
            {
                let row = rows.remove(from);
                rows.insert(to, row);
            }
            Action::InsertColumn(index) if index <= columns => {
                for row in &mut rows {
                    row.insert(index, String::new());
                }
                alignments.insert(index, Alignment::None);
            }
            Action::RemoveColumn(index) if columns > 1 && index < columns => {
                for row in &mut rows {
                    row.remove(index);
                }
                alignments.remove(index);
            }
            Action::MoveColumn { from, to } if from < columns && to < columns => {
                for row in &mut rows {
                    let cell = row.remove(from);
                    row.insert(to, cell);
                }
                let alignment = alignments.remove(from);
                alignments.insert(to, alignment);
            }
            Action::Clear {
                rows: ref selected,
                columns: ref chosen,
            } if !selected.is_empty()
                && !chosen.is_empty()
                && selected.end <= rows.len()
                && chosen.end <= columns =>
            {
                for row in &mut rows[selected.clone()] {
                    for cell in &mut row[chosen.clone()] {
                        cell.clear();
                    }
                }
            }
            Action::RemoveRows(ref selected)
                if selected.start > 0
                    && selected.start < selected.end
                    && selected.end <= rows.len() =>
            {
                rows.drain(selected.clone());
            }
            Action::RemoveColumns(ref selected)
                if selected.start < selected.end
                    && selected.end <= columns
                    && selected.len() < columns =>
            {
                for row in &mut rows {
                    row.drain(selected.clone());
                }
                alignments.drain(selected.clone());
            }
            Action::Fill {
                row,
                column,
                ref cells,
            } if row < rows.len()
                && column < columns
                && !cells.is_empty()
                && cells.len() <= MAX_PASTE_LINES
                && cells.iter().all(|line| line.len() <= MAX_PASTE_COLUMNS) =>
            {
                let wanted = columns.max(column + cells.iter().map(Vec::len).max().unwrap_or(0));
                alignments.resize(wanted, Alignment::None);
                rows.resize(rows.len().max(row + cells.len()), Vec::new());
                for line in &mut rows {
                    if line.len() < wanted {
                        line.resize(wanted, String::new());
                    }
                }
                for (offset, line) in cells.iter().enumerate() {
                    for (index, text) in line.iter().enumerate() {
                        rows[row + offset][column + index] = encode_pipes(text);
                    }
                }
            }
            Action::Align { column, alignment } if column < columns => {
                // Alignment changes need not reformat any content row.
                let marker = match alignment {
                    Alignment::Left => ":---",
                    Alignment::Center => ":---:",
                    Alignment::Right => "---:",
                    Alignment::None => "---",
                };
                source.replace_range(self.separator.cells[column].clone(), marker);
                return Ok(());
            }
            Action::Sort { column, descending } if column < columns => {
                let mut entries: Vec<_> = rows
                    .drain(1..)
                    .map(|row| {
                        let key = sort_text(&row[column]);
                        let number = key
                            .trim()
                            .parse::<f64>()
                            .ok()
                            .filter(|number| number.is_finite());
                        (row, key.to_lowercase(), number)
                    })
                    .collect();
                let numeric = entries.iter().all(|(_, _, number)| number.is_some());
                entries.sort_by(|a, b| {
                    let order = if numeric {
                        a.2.unwrap().total_cmp(&b.2.unwrap())
                    } else {
                        a.1.cmp(&b.1)
                    };
                    if descending { order.reverse() } else { order }
                });
                rows.extend(entries.into_iter().map(|(row, _, _)| row));
            }
            _ => {
                return Err(
                    "Essa operação na tabela removeria o cabeçalho ou a última coluna, ou usa uma célula que não existe.",
                );
            }
        }
        let replacement = self.render(source, &rows, &alignments, false);
        source.replace_range(self.range.clone(), &replacement);
        Ok(())
    }

    /// The table's own text with every pipe aligned: cells padded to the widest of their
    /// column, the separator as wide as the column. Cell contents are untouched, so a table
    /// that is already aligned comes back byte for byte.
    pub fn formatted(&self, source: &str) -> String {
        let columns = self.alignments.len();
        let rows: Vec<Vec<String>> = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                (0..row.cells.len().max(columns))
                    .map(|column| self.cell(source, index, column).to_owned())
                    .collect()
            })
            .collect();
        self.render(source, &rows, &self.alignments, true)
    }

    /// The Markdown for `rows` (header first) inside this table's own prefix (a quote or list
    /// marker) and line ending. `padded` aligns the pipes; otherwise each cell is `| text |`.
    fn render(
        &self,
        source: &str,
        rows: &[Vec<String>],
        alignments: &[Alignment],
        padded: bool,
    ) -> String {
        let newline = if self.separator.ending.is_empty() {
            "\n"
        } else {
            &self.separator.ending
        };
        let trailing = source[self.range.clone()].ends_with('\n');
        let mut text = render_rows(
            &self.rows[0].prefix,
            &self.separator.prefix,
            rows,
            alignments,
            newline,
            padded,
        );
        if !trailing {
            text.truncate(text.len() - newline.len());
        }
        text
    }
}

/// Header, separator and body, each line closed by `newline`; later lines use `continuation`.
fn render_rows(
    first: &str,
    continuation: &str,
    rows: &[Vec<String>],
    alignments: &[Alignment],
    newline: &str,
    padded: bool,
) -> String {
    let columns = rows
        .iter()
        .map(Vec::len)
        .max()
        .unwrap_or(0)
        .max(alignments.len());
    // The separator needs three characters (`---`) to be a separator at all.
    let widths: Vec<usize> = (0..columns)
        .map(|column| {
            if !padded {
                return 0;
            }
            rows.iter()
                .filter_map(|row| row.get(column))
                .map(|cell| display_width(cell))
                .max()
                .unwrap_or(0)
                .max(3)
        })
        .collect();
    let alignment = |column: usize| alignments.get(column).copied().unwrap_or(Alignment::None);
    let mut output = String::new();
    for (index, row) in rows.iter().enumerate() {
        output.push_str(if index == 0 { first } else { continuation });
        output.push('|');
        if padded {
            for (column, width) in widths.iter().enumerate() {
                let cell = row.get(column).map_or("", String::as_str);
                output.push(' ');
                push_aligned(&mut output, cell, *width, alignment(column));
                output.push_str(" |");
            }
        } else {
            for cell in row {
                output.push(' ');
                output.push_str(cell);
                output.push_str(" |");
            }
        }
        output.push_str(newline);
        if index == 0 {
            output.push_str(continuation);
            output.push('|');
            for (column, width) in widths.iter().enumerate().take(alignments.len()) {
                output.push(' ');
                push_marker(&mut output, alignment(column), *width);
                output.push_str(" |");
            }
            output.push_str(newline);
        }
    }
    output
}

/// `cell` inside `width` columns, placed as `alignment` says.
fn push_aligned(output: &mut String, cell: &str, width: usize, alignment: Alignment) {
    let spare = width.saturating_sub(display_width(cell));
    let (before, after) = match alignment {
        Alignment::Right => (spare, 0),
        Alignment::Center => (spare / 2, spare - spare / 2),
        Alignment::Left | Alignment::None => (0, spare),
    };
    output.extend(std::iter::repeat_n(' ', before));
    output.push_str(cell);
    output.extend(std::iter::repeat_n(' ', after));
}

/// The separator cell of one column; `width` 0 gives the short form (`---`, `:---:`).
fn push_marker(output: &mut String, alignment: Alignment, width: usize) {
    let (left, right) = match alignment {
        Alignment::Left => (true, false),
        Alignment::Center => (true, true),
        Alignment::Right => (false, true),
        Alignment::None => (false, false),
    };
    // The short form keeps three dashes between the colons.
    let width = if width == 0 {
        3 + usize::from(left) + usize::from(right)
    } else {
        width
    };
    output.push_str(if left { ":" } else { "-" });
    output.extend(std::iter::repeat_n('-', width.saturating_sub(2)));
    output.push_str(if right { ":" } else { "-" });
}

/// Terminal columns of `text`: wide East Asian and emoji characters take two, combining marks
/// none. Good enough to line pipes up in a monospaced editor without a width table.
fn display_width(text: &str) -> usize {
    text.chars()
        .map(|character| match character as u32 {
            0x0300..=0x036F | 0x200B..=0x200F | 0xFE00..=0xFE0F => 0,
            0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1F64F
            | 0x1F900..=0x1F9FF
            | 0x20000..=0x3FFFD => 2,
            _ => 1,
        })
        .sum()
}

impl Row {
    fn parse(source: &str, content_start: usize) -> Option<Self> {
        let start = source
            .get(..content_start)?
            .rfind('\n')
            .map_or(0, |i| i + 1);
        let end = source[content_start..]
            .find('\n')
            .map_or(source.len(), |i| content_start + i + 1);
        let content_end = source[..end].trim_end_matches(['\n', '\r']).len();
        let content = source.get(content_start..content_end)?;
        let trimmed = content.trim_matches([' ', '\t']);
        let mut first =
            content_start + content.len() - content.trim_start_matches([' ', '\t']).len();
        let mut last = first + trimmed.len();
        if source.as_bytes().get(first) == Some(&b'|') {
            first += 1;
        }
        if last > first && source.as_bytes()[last - 1] == b'|' && !escaped(source, last - 1) {
            last -= 1;
        }
        let mut fields = Vec::new();
        let mut cell_start = first;
        for (offset, character) in source[first..last].char_indices() {
            let byte = first + offset;
            if character == '|' && !escaped(source, byte) {
                fields.push(cell_start..byte);
                cell_start = byte + 1;
            }
        }
        fields.push(cell_start..last);
        Some(Self {
            cells: fields
                .iter()
                .map(|range| trim_range(source, range.clone()))
                .collect(),
            fields,
            line: start..end,
            prefix: source[start..content_start].into(),
            ending: source[content_end..end].into(),
        })
    }
}

fn trim_range(source: &str, range: Range<usize>) -> Range<usize> {
    let text = &source[range.clone()];
    if text.trim_matches([' ', '\t']).is_empty() {
        let middle = range.start + text.len() / 2;
        return middle..middle;
    }
    let start = range.start + text.len() - text.trim_start_matches([' ', '\t']).len();
    start..start + text.trim_matches([' ', '\t']).len()
}

fn escaped(source: &str, byte: usize) -> bool {
    source[..byte]
        .bytes()
        .rev()
        .take_while(|byte| *byte == b'\\')
        .count()
        % 2
        == 1
}

fn encode_pipes(text: &str) -> String {
    let mut result = String::new();
    for (index, character) in text.char_indices() {
        if character == '|' && !escaped(text, index) {
            result.push('\\');
        }
        result.push(character);
    }
    result
}

fn decode_pipes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\\' && chars.peek() == Some(&'|') {
            continue;
        }
        result.push(character);
        if character == '\\' && chars.peek() == Some(&'\\') {
            result.push(chars.next().unwrap());
        }
    }
    result
}

fn write_row<'a>(output: &mut String, prefix: &str, cells: impl Iterator<Item = &'a str>) {
    output.push_str(prefix);
    output.push('|');
    for cell in cells {
        output.push(' ');
        output.push_str(cell);
        output.push_str(" |");
    }
}

fn sort_text(source: &str) -> String {
    let mut display = decode_pipes(source);
    for (range, target) in crate::vault::wiki_spans(&display).into_iter().rev() {
        display.replace_range(
            range,
            target
                .split_once('|')
                .map_or(target.as_str(), |(_, label)| label),
        );
    }
    pulldown_cmark::Parser::new_ext(&display, pulldown_cmark::Options::ENABLE_STRIKETHROUGH)
        .filter_map(|event| match event {
            pulldown_cmark::Event::Text(text) | pulldown_cmark::Event::Code(text) => {
                Some(text.into_string())
            }
            pulldown_cmark::Event::SoftBreak | pulldown_cmark::Event::HardBreak => Some(" ".into()),
            _ => None,
        })
        .collect()
}

/// A paste is a spreadsheet range, not a book: past these it stays plain text.
const MAX_PASTE_LINES: usize = 2000;
const MAX_PASTE_COLUMNS: usize = 100;
/// The size limits of a table made from the palette.
pub const MAX_NEW_COLUMNS: usize = 30;
pub const MAX_NEW_ROWS: usize = 500;

/// The Markdown for an empty table of `columns` x `rows` lines (the header counts as one),
/// pipes aligned, without a final line break. `None` when the size is out of range.
pub fn blank_table(columns: usize, rows: usize) -> Option<String> {
    if !(1..=MAX_NEW_COLUMNS).contains(&columns) || !(1..=MAX_NEW_ROWS).contains(&rows) {
        return None;
    }
    let grid = vec![vec![String::new(); columns]; rows];
    Some(grid_markdown(&grid))
}

/// "3x4", "3 x 4", "3×4" or "3,4": columns, then lines.
pub fn parse_size(input: &str) -> Option<(usize, usize)> {
    let (columns, rows) = input.split_once(['x', 'X', '×', ',', ';'])?;
    Some((columns.trim().parse().ok()?, rows.trim().parse().ok()?))
}

/// A grid of cells as a padded Markdown table, the first line as the header and no final break.
pub fn grid_markdown(grid: &[Vec<String>]) -> String {
    let columns = grid.iter().map(Vec::len).max().unwrap_or(0);
    let rows: Vec<Vec<String>> = grid
        .iter()
        .map(|line| {
            let mut line: Vec<String> = line.iter().map(|text| encode_pipes(text)).collect();
            line.resize(columns, String::new());
            line
        })
        .collect();
    let mut text = render_rows("", "", &rows, &vec![Alignment::None; columns], "\n", true);
    text.pop();
    text
}

/// Every table of `source` with its pipes aligned; `None` when nothing would change.
pub fn format_all(source: &str) -> Option<String> {
    format_where(source, |_| true)
}

/// Only the table that holds the byte `caret` (its first or last byte included).
pub fn format_at(source: &str, caret: usize) -> Option<String> {
    format_where(source, |range| range.start <= caret && caret <= range.end)
}

fn format_where(source: &str, wanted: impl Fn(&Range<usize>) -> bool) -> Option<String> {
    let mut result = source.to_owned();
    // Back to front: an earlier table keeps its offsets while a later one changes length.
    for table in crate::editor::Markdown::parse(source)
        .tables
        .iter()
        .rev()
        .filter(|table| wanted(&table.range))
    {
        result.replace_range(table.range.clone(), &table.formatted(source));
    }
    (result != source).then_some(result)
}

/// The line breaks to put around a block of `block` text that replaces `start..end` of `source`
/// so that it stands alone as a paragraph: `(before, after)`.
pub fn block_padding(source: &str, start: usize, end: usize) -> (&'static str, &'static str) {
    let head = &source[..start];
    let tail = &source[end..];
    let before = if head.is_empty() || head.ends_with("\n\n") {
        ""
    } else if head.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    let after = if tail.is_empty() {
        "\n"
    } else if tail.starts_with("\n\n") {
        ""
    } else if tail.starts_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    (before, after)
}

/// Cells of delimited text: records end at a line break, fields at `delimiter`, and a field
/// opened with `"` runs (line breaks included) to the closing quote, `""` being a quote. Line
/// breaks inside a field become `<br>`; every cell is trimmed.
fn split_records(text: &str, delimiter: char) -> Vec<Vec<String>> {
    let mut records = vec![Vec::new()];
    let mut field = String::new();
    let mut quoted = false;
    let mut at_start = true;
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        if quoted {
            match character {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\r' if chars.peek() == Some(&'\n') => {}
                '\n' | '\r' => field.push_str("<br>"),
                _ => field.push(character),
            }
            continue;
        }
        match character {
            '"' if at_start => {
                quoted = true;
                at_start = false;
            }
            _ if character == delimiter => {
                records.last_mut().unwrap().push(field.trim().to_owned());
                field.clear();
                at_start = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' | '\r' => {
                records.last_mut().unwrap().push(field.trim().to_owned());
                field.clear();
                records.push(Vec::new());
                at_start = true;
            }
            _ => {
                field.push(character);
                at_start = at_start && character.is_whitespace();
            }
        }
    }
    if !field.is_empty() || !records.last().unwrap().is_empty() {
        records.last_mut().unwrap().push(field.trim().to_owned());
    }
    if records.last().is_some_and(Vec::is_empty) {
        records.pop();
    }
    records
}

/// A rectangular grid when `text` is a range copied from a spreadsheet: tab-separated, at least
/// `min_lines` lines, every line with the same number of cells (two or more) and at least one
/// non-empty cell in the first column.
fn tab_grid(text: &str, min_lines: usize) -> Option<Vec<Vec<String>>> {
    if !text.contains('\t') {
        return None;
    }
    let grid = split_records(text, '\t');
    let columns = grid.first()?.len();
    (columns >= 2
        && grid.len() >= min_lines
        && grid.len() <= MAX_PASTE_LINES
        && columns <= MAX_PASTE_COLUMNS
        && grid.iter().all(|line| line.len() == columns)
        && grid.iter().any(|line| !line[0].is_empty()))
    .then_some(grid)
}

/// The block pasted at the cursor of a note: two or more spreadsheet lines become a table. Plain
/// prose, a single line and code indented with tabs (lines with an empty first cell, or a
/// different count of tabs) stay as they are.
pub fn paste_grid(text: &str) -> Option<Vec<Vec<String>>> {
    tab_grid(text, 2)
}

/// What pasting `text` into a table cell fills: any tab-separated range, even one line.
pub fn paste_cells(text: &str) -> Option<Vec<Vec<String>>> {
    tab_grid(text, 1)
}

/// The grid of a selection converted by the palette: tab, `;` or `,` separated text, choosing
/// the delimiter that gives every line the same number of cells (two or more), tab first.
pub fn selection_grid(text: &str) -> Option<Vec<Vec<String>>> {
    ['\t', ';', ','].into_iter().find_map(|delimiter| {
        let grid = split_records(text, delimiter);
        let columns = grid.first()?.len();
        (columns >= 2
            && grid.len() <= MAX_PASTE_LINES
            && columns <= MAX_PASTE_COLUMNS
            && grid.iter().all(|line| line.len() == columns))
        .then_some(grid)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(source: &str) -> Table {
        let mut tables = crate::editor::Markdown::parse(source).tables;
        assert_eq!(tables.len(), 1, "{source:?}");
        tables.remove(0)
    }

    fn cells(rows: &[&[&str]]) -> Vec<Vec<String>> {
        rows.iter()
            .map(|row| row.iter().map(|cell| (*cell).to_owned()).collect())
            .collect()
    }

    #[test]
    fn a_new_table_has_the_asked_size_with_aligned_pipes() {
        assert_eq!(
            blank_table(3, 2).unwrap(),
            "|     |     |     |\n| --- | --- | --- |\n|     |     |     |"
        );
        assert_eq!(blank_table(1, 1).unwrap(), "|     |\n| --- |");
        let parsed = table(&blank_table(4, 6).unwrap());
        assert_eq!((parsed.alignments.len(), parsed.rows.len()), (4, 6));
        for (columns, rows) in [
            (0, 2),
            (2, 0),
            (MAX_NEW_COLUMNS + 1, 2),
            (2, MAX_NEW_ROWS + 1),
        ] {
            assert!(blank_table(columns, rows).is_none(), "{columns}x{rows}");
        }
    }

    #[test]
    fn the_size_accepts_the_usual_ways_to_write_it() {
        for input in ["3x4", "3 x 4", " 3X4 ", "3×4", "3,4", "3;4"] {
            assert_eq!(parse_size(input), Some((3, 4)), "{input:?}");
        }
        for input in ["", "3", "x4", "3x", "três x 4", "3x4x5", "-1x2"] {
            assert_eq!(parse_size(input), None, "{input:?}");
        }
    }

    #[test]
    fn formatting_aligns_the_pipes_by_the_alignment_of_each_column() {
        let source = "# T\n\n| Item | Valor | Obs |\n| :--- | ---: | :-: |\n| Maçã | 12 | ok |\n| Abacaxi grande | 7,5 | sem açúcar |\n\nFim\n";
        let expected = "# T\n\n| Item           | Valor |    Obs     |\n| :------------- | ----: | :--------: |\n| Maçã           |    12 |     ok     |\n| Abacaxi grande |   7,5 | sem açúcar |\n\nFim\n";
        assert_eq!(format_all(source).as_deref(), Some(expected));
        // Already aligned: nothing to do, and a second pass is the identity.
        assert_eq!(format_all(expected), None);
    }

    #[test]
    fn formatting_keeps_the_prefix_the_line_ending_and_the_content_of_each_cell() {
        let source =
            "> | a | b |\r\n> | --- | --- |\r\n> | `x\\|y` | [[Home\\|Alias]] |\r\n\r\nresto";
        let formatted = format_all(source).unwrap();
        assert_eq!(
            formatted,
            "> | a      | b               |\r\n> | ------ | --------------- |\r\n> | `x\\|y` | [[Home\\|Alias]] |\r\n\r\nresto"
        );
        // The same cells come out when the result is read back.
        let parsed = table(&formatted);
        assert_eq!(parsed.cell_text(&formatted, 1, 0), "`x|y`");
    }

    #[test]
    fn formatting_pads_short_rows_and_counts_wide_characters_as_two_columns() {
        let source = "| 名前 | b |\n| --- | --- |\n| x |\n";
        assert_eq!(
            format_all(source).as_deref(),
            Some("| 名前 | b   |\n| ---- | --- |\n| x    |     |\n")
        );
    }

    #[test]
    fn formatting_at_the_caret_touches_only_that_table() {
        let first = "|a|b|\n|-|-|\n|1|2|\n";
        let second = "| c | d |\n| - | - |\n| 3 | 4 |\n";
        let source = format!("{first}\ntexto\n\n{second}");
        let caret_in_second = source.len() - 3;
        let only_second = format_at(&source, caret_in_second).unwrap();
        assert!(only_second.starts_with(first), "{only_second}");
        assert!(only_second.contains("| c   | d   |"), "{only_second}");
        assert_eq!(
            format_at(&source, first.len() + 2),
            None,
            "caret in the prose"
        );
        let both = format_all(&source).unwrap();
        assert!(both.starts_with("| a   | b   |\n| --- | --- |\n| 1   | 2   |\n"));
    }

    #[test]
    fn a_spreadsheet_range_becomes_a_table_but_prose_and_code_do_not() {
        let excel =
            "Nome\tQtd\tObs\r\nAna\t3\t\"linha 1\nlinha 2\"\r\nBia\t\t\"diz \"\"oi\"\"\"\r\n";
        let grid = paste_grid(excel).unwrap();
        assert_eq!(
            grid,
            cells(&[
                &["Nome", "Qtd", "Obs"],
                &["Ana", "3", "linha 1<br>linha 2"],
                &["Bia", "", "diz \"oi\""],
            ])
        );
        assert_eq!(
            grid_markdown(&grid),
            "| Nome | Qtd | Obs                |\n| ---- | --- | ------------------ |\n| Ana  | 3   | linha 1<br>linha 2 |\n| Bia  |     | diz \"oi\"           |"
        );
        for not_a_table in [
            "só texto, com vírgulas",
            "uma\tlinha só\n",
            "a\tb\nc\td\te\n",
            "\tum\n\tdois\n",
            "if x {\n\tfoo();\n\t\tbar();\n}\n",
            "",
        ] {
            assert_eq!(paste_grid(not_a_table), None, "{not_a_table:?}");
        }
        // Inside a cell even one copied line fills cells.
        assert_eq!(paste_cells("a\tb\n"), Some(cells(&[&["a", "b"]])));
        assert_eq!(paste_cells("a\nb\n"), None);
    }

    #[test]
    fn pipes_in_pasted_cells_are_escaped_in_the_table() {
        let grid = cells(&[&["a|b", "c"], &["1", "2"]]);
        let markdown = grid_markdown(&grid);
        let parsed = table(&markdown);
        assert_eq!(parsed.cell_text(&markdown, 0, 0), "a|b");
        assert_eq!(parsed.rows.len(), 2);
    }

    #[test]
    fn a_selection_converts_from_tab_semicolon_or_comma_in_that_order() {
        assert_eq!(
            selection_grid("a\tb,c\n1\t2,3"),
            Some(cells(&[&["a", "b,c"], &["1", "2,3"]]))
        );
        assert_eq!(
            selection_grid("a;b\n1;2"),
            Some(cells(&[&["a", "b"], &["1", "2"]]))
        );
        assert_eq!(
            selection_grid("nome,nota\n\"Silva, Ana\",9\nBia,8\n"),
            Some(cells(&[
                &["nome", "nota"],
                &["Silva, Ana", "9"],
                &["Bia", "8"]
            ]))
        );
        assert_eq!(selection_grid("a,b\n1,2,3"), None, "ragged lines");
        assert_eq!(selection_grid("sem separador"), None);
    }

    #[test]
    fn a_block_gets_the_blank_lines_it_needs_around_it() {
        let cases = [
            ("", 0, 0, "", "\n"),
            ("texto", 5, 5, "\n\n", "\n"),
            ("texto\n", 6, 6, "\n", "\n"),
            ("texto\n\n", 7, 7, "", "\n"),
            ("antes\n\ndepois", 7, 7, "", "\n\n"),
            ("antes\n\n\ndepois", 7, 7, "", "\n"),
            ("antes depois", 5, 6, "\n\n", "\n\n"),
            ("antes\ndepois", 6, 6, "\n", "\n\n"),
            ("antes\n\ndepois", 5, 5, "\n\n", ""),
        ];
        for (source, start, end, before, after) in cases {
            assert_eq!(
                block_padding(source, start, end),
                (before, after),
                "{source:?} {start}..{end}"
            );
        }
    }

    #[test]
    fn pasting_a_block_of_cells_grows_the_table_and_replaces_what_was_there() {
        let base = "| A | B |\n| --- | --- |\n| 1 | 2 |\n\nfim\n";
        let model = table(base);
        let mut source = base.to_owned();
        model
            .apply(
                &mut source,
                Action::Fill {
                    row: 1,
                    column: 1,
                    cells: cells(&[&["x|y", "z"], &["p", "q"], &["r", "s"]]),
                },
            )
            .unwrap();
        assert_eq!(
            source,
            "| A | B |  |\n| --- | --- | --- |\n| 1 | x\\|y | z |\n|  | p | q |\n|  | r | s |\n\nfim\n"
        );
        let grown = table(&source);
        assert_eq!((grown.alignments.len(), grown.rows.len()), (3, 4));
        assert_eq!(grown.cell_text(&source, 1, 1), "x|y");
        assert_eq!(grown.cell_text(&source, 3, 2), "s");
    }

    #[test]
    fn a_paste_that_does_not_fit_is_refused_without_touching_the_source() {
        let base = "| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let model = table(base);
        for action in [
            Action::Fill {
                row: 9,
                column: 0,
                cells: cells(&[&["x"]]),
            },
            Action::Fill {
                row: 0,
                column: 5,
                cells: cells(&[&["x"]]),
            },
            Action::Fill {
                row: 0,
                column: 0,
                cells: Vec::new(),
            },
        ] {
            let mut source = base.to_owned();
            assert!(model.apply(&mut source, action).is_err());
            assert_eq!(source, base);
        }
    }
}
