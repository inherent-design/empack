//! Native widgets use one cancellable reader and dialoguer's existing theme/matcher.
use super::unix_input::{InputMode, read_key};
use crate::application::process_runtime::Cancellation;
use anyhow::{Result, ensure};
use console::{Key, Term, measure_text_width, truncate_str};
use dialoguer::theme::{SimpleTheme, Theme};
use fuzzy_matcher::{FuzzyMatcher, skim::SkimMatcherV2};
use std::os::fd::RawFd;

pub(super) fn text(prompt: &str, default: String, token: &Cancellation) -> Result<String> {
    text_on(libc::STDIN_FILENO, &Term::stderr(), prompt, default, token)
}

pub(super) fn select(prompt: &str, options: &[&str], token: &Cancellation) -> Result<usize> {
    select_on(libc::STDIN_FILENO, &Term::stderr(), prompt, options, token)
}

pub(super) fn fuzzy(
    prompt: &str,
    options: &[String],
    token: &Cancellation,
) -> Result<Option<usize>> {
    fuzzy_on(libc::STDIN_FILENO, &Term::stderr(), prompt, options, token)
}

fn with_input<T>(
    input: RawFd,
    terminal: &Term,
    token: &Cancellation,
    action: impl FnOnce(&mut Screen<'_>) -> Result<T>,
) -> Result<T> {
    token.check()?;
    ensure!(terminal.is_term(), "Interactive output is not a terminal");
    let mut mode = InputMode::capture(input)?;
    let mut screen = Screen {
        terminal,
        frame: Frame::Empty,
    };
    // Mode is owned before any prompt is visible. No detached input task or
    // process-wide signal changes are needed to wake a cancelled prompt.
    let result = action(&mut screen).and_then(|value| {
        token.check()?;
        Ok(value)
    });
    let display = screen
        .clear()
        .and_then(|()| terminal.show_cursor())
        .and_then(|()| terminal.flush());
    let restore = mode.restore();
    let cleanup = match (display, restore) {
        (Err(display), Err(restore)) => {
            Err(restore.context(format!("Prompt display cleanup failed: {display}")))
        }
        (Err(display), Ok(())) => Err(display.into()),
        (Ok(()), restore) => restore,
    };
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) => Err(error),
        (Ok(_), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => {
            Err(error.context(format!("Interactive terminal cleanup failed: {cleanup:#}")))
        }
    }
}

enum Frame {
    Empty,
    Input,
    Menu(usize),
}
struct Screen<'a> {
    terminal: &'a Term,
    frame: Frame,
}
impl Screen<'_> {
    fn clear(&mut self) -> std::io::Result<()> {
        match self.frame {
            Frame::Empty => (),
            Frame::Input => self.terminal.clear_line()?,
            Frame::Menu(lines) => self.terminal.clear_last_lines(lines)?,
        }
        self.frame = Frame::Empty;
        Ok(())
    }

    fn menu(&mut self, lines: &[String]) -> Result<()> {
        self.clear()?;
        self.terminal.hide_cursor()?;
        let width = usize::from(self.terminal.size().1).saturating_sub(1).max(1);
        for line in lines {
            // Keep each owned row within the viewport; selection still returns
            // the full original value/index rather than its display spelling.
            let line = line.replace(['\r', '\n'], " ");
            self.terminal.write_line(&truncate_str(&line, width, "…"))?;
            if let Frame::Menu(ref mut count) = self.frame {
                *count += 1;
            } else {
                self.frame = Frame::Menu(1);
            }
        }
        self.terminal.flush()?;
        Ok(())
    }

    fn input(&mut self, prefix: &str, editor: &Editor) -> Result<()> {
        self.clear()?;
        self.frame = Frame::Input;
        let input: String = editor.characters.iter().collect();
        let head: String = editor.characters[..editor.cursor].iter().collect();
        let full = format!("{prefix}{input}");
        let cursor = prefix.len() + head.len();
        let width = usize::from(self.terminal.size().1).saturating_sub(1).max(2);
        let mut start = 0;
        // Scroll horizontally by complete Unicode scalars, using display width
        // rather than UTF-8 byte offsets to place the cursor.
        while measure_text_width(&full[start..cursor]) > width.saturating_sub(1) {
            start += full[start..]
                .chars()
                .next()
                .expect("text before cursor")
                .len_utf8();
        }
        let marker = if start == 0 { "" } else { "…" };
        let text = truncate_str(&full[start..], width - measure_text_width(marker), "");
        self.terminal.write_str(marker)?;
        self.terminal.write_str(&text)?;
        let position = measure_text_width(marker) + measure_text_width(&full[start..cursor]);
        let end = measure_text_width(marker) + measure_text_width(&text);
        self.terminal
            .move_cursor_left(end.saturating_sub(position))?;
        self.terminal.flush()?;
        Ok(())
    }

    fn report(&mut self, prompt: &str, answer: &str) -> Result<()> {
        self.clear()?;
        let mut rendered = String::new();
        SimpleTheme.format_input_prompt_selection(&mut rendered, prompt, answer)?;
        self.terminal.write_line(&rendered)?;
        Ok(())
    }
}
impl Drop for Screen<'_> {
    fn drop(&mut self) {
        let _ = self.clear();
        let _ = self.terminal.show_cursor();
    }
}

#[derive(Default)]
struct Editor {
    characters: Vec<char>,
    cursor: usize,
}
impl Editor {
    fn edit(&mut self, key: &Key) -> bool {
        match key {
            Key::Char(value) if !value.is_control() => {
                self.characters.insert(self.cursor, *value);
                self.cursor += 1;
            }
            Key::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.characters.remove(self.cursor);
            }
            Key::Del if self.cursor < self.characters.len() => {
                self.characters.remove(self.cursor);
            }
            Key::ArrowLeft if self.cursor > 0 => self.cursor -= 1,
            Key::ArrowRight if self.cursor < self.characters.len() => self.cursor += 1,
            Key::Home => self.cursor = 0,
            Key::End => self.cursor = self.characters.len(),
            Key::UnknownEscSeq(sequence) if sequence == &['b'] => {
                while self.cursor > 0 && self.characters[self.cursor - 1].is_whitespace() {
                    self.cursor -= 1;
                }
                while self.cursor > 0 && !self.characters[self.cursor - 1].is_whitespace() {
                    self.cursor -= 1;
                }
            }
            Key::UnknownEscSeq(sequence) if sequence == &['f'] => {
                while self.cursor < self.characters.len()
                    && !self.characters[self.cursor].is_whitespace()
                {
                    self.cursor += 1;
                }
                while self.cursor < self.characters.len()
                    && self.characters[self.cursor].is_whitespace()
                {
                    self.cursor += 1;
                }
            }
            _ => return false,
        }
        true
    }
    fn value(&self) -> String {
        self.characters.iter().collect()
    }
}

fn text_on(
    input: RawFd,
    terminal: &Term,
    prompt: &str,
    default: String,
    token: &Cancellation,
) -> Result<String> {
    with_input(input, terminal, token, |screen| {
        let mut prefix = String::new();
        SimpleTheme.format_input_prompt(&mut prefix, prompt, Some(&default))?;
        let mut editor = Editor::default();
        loop {
            token.check()?;
            screen.input(&prefix, &editor)?;
            let key = read_key(input, token)?;
            if key == Key::Enter {
                token.check()?;
                let value = if editor.characters.is_empty() {
                    default
                } else {
                    editor.value()
                };
                screen.report(prompt, &value)?;
                return Ok(value);
            }
            editor.edit(&key);
        }
    })
}

fn select_on(
    input: RawFd,
    terminal: &Term,
    prompt: &str,
    options: &[&str],
    token: &Cancellation,
) -> Result<usize> {
    ensure!(!options.is_empty(), "Cannot select from an empty list");
    with_input(input, terminal, token, |screen| {
        let mut selected = None;
        loop {
            token.check()?;
            let capacity = usize::from(terminal.size().0).saturating_sub(2).max(1);
            let first = selected.unwrap_or(0) / capacity * capacity;
            let mut title = String::new();
            SimpleTheme.format_select_prompt(&mut title, prompt)?;
            let mut lines = vec![title];
            for (index, option) in options.iter().enumerate().skip(first).take(capacity) {
                token.check()?;
                let mut line = String::new();
                SimpleTheme.format_select_prompt_item(
                    &mut line,
                    option,
                    selected == Some(index),
                )?;
                lines.push(line);
            }
            screen.menu(&lines)?;
            match read_key(input, token)? {
                Key::ArrowDown | Key::Tab | Key::Char('j') => {
                    selected = Some(selected.map_or(0, |i| (i + 1) % options.len()))
                }
                Key::ArrowUp | Key::BackTab | Key::Char('k') => {
                    selected = Some(selected.map_or(options.len() - 1, |i| {
                        (i + options.len() - 1) % options.len()
                    }))
                }
                Key::ArrowRight | Key::PageDown | Key::Char('l') => {
                    selected = Some((first + capacity).min(options.len() - 1))
                }
                Key::ArrowLeft | Key::PageUp | Key::Char('h') => {
                    selected = Some(first.saturating_sub(capacity))
                }
                Key::Home => selected = Some(0),
                Key::End => selected = Some(options.len() - 1),
                Key::Enter | Key::Char(' ') if selected.is_some() => {
                    token.check()?;
                    let index = selected.expect("selection is present");
                    screen.report(prompt, options[index])?;
                    return Ok(index);
                }
                _ => (),
            }
        }
    })
}

fn fuzzy_on(
    input: RawFd,
    terminal: &Term,
    prompt: &str,
    options: &[String],
    token: &Cancellation,
) -> Result<Option<usize>> {
    ensure!(!options.is_empty(), "Cannot search an empty list");
    with_input(input, terminal, token, |screen| {
        let matcher = SkimMatcherV2::default();
        let mut editor = Editor::default();
        let mut selected: Option<usize> = None;
        loop {
            token.check()?;
            let query = editor.value();
            let mut matching = Vec::new();
            for (index, value) in options.iter().enumerate() {
                token.check()?;
                if let Some(score) = matcher.fuzzy_match(value, &query) {
                    matching.push((index, score));
                }
            }
            matching.sort_by(|(ai, a), (bi, b)| b.cmp(a).then(ai.cmp(bi)));
            selected = selected.filter(|index| *index < matching.len());
            let capacity = usize::from(terminal.size().0).saturating_sub(2).clamp(1, 6);
            let first = selected.unwrap_or(0) / capacity * capacity;
            let cursor_bytes: usize = editor.characters[..editor.cursor]
                .iter()
                .map(|c| c.len_utf8())
                .sum();
            let mut title = String::new();
            SimpleTheme.format_fuzzy_select_prompt(&mut title, prompt, &query, cursor_bytes)?;
            let mut lines = vec![title];
            for (row, (index, _)) in matching.iter().enumerate().skip(first).take(capacity) {
                token.check()?;
                let mut line = String::new();
                SimpleTheme.format_fuzzy_select_prompt_item(
                    &mut line,
                    &options[*index],
                    selected == Some(row),
                    true,
                    &matcher,
                    &query,
                )?;
                lines.push(line);
            }
            if matching.is_empty() {
                lines.push("  No matches".into());
            }
            screen.menu(&lines)?;
            let key = read_key(input, token)?;
            match key {
                Key::Escape => return Ok(None),
                Key::ArrowDown | Key::Tab if !matching.is_empty() => {
                    selected = Some(selected.map_or(0, |i| (i + 1) % matching.len()))
                }
                Key::ArrowUp | Key::BackTab if !matching.is_empty() => {
                    selected = Some(selected.map_or(matching.len() - 1, |i| {
                        (i + matching.len() - 1) % matching.len()
                    }))
                }
                Key::PageDown if !matching.is_empty() => {
                    selected = Some((first + capacity).min(matching.len() - 1))
                }
                Key::PageUp if !matching.is_empty() => {
                    selected = Some(first.saturating_sub(capacity))
                }
                Key::Enter if selected.is_some() => {
                    token.check()?;
                    let index = matching[selected.expect("selection is present")].0;
                    screen.report(prompt, &options[index])?;
                    return Ok(Some(index));
                }
                _ => {
                    let before = editor.value();
                    if editor.edit(&key) && editor.value() != before {
                        selected = Some(0);
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests;
