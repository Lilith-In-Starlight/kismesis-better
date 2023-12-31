use std::fmt::Debug;

use crate::kismesis::{FileRef, KisID, Kismesis};

use super::{
	errors::{ErrorKind, ErrorState, StatelessError},
	html::ScopedError,
	lexer::Token,
	parser::{errors::Hint, state::TokenPos},
};
use colored::*;

pub struct DrawingInfo<'a> {
	pub(crate) line_number_length: usize,
	pub(crate) scope: &'a FileRef,
	pub(crate) lines: Vec<(usize, &'a [Token])>,
	pub(crate) line_offset: (usize, usize),
	pub(crate) hint: bool,
}

#[derive(Debug)]
enum ReportingError {
	InvalidKismesisID,
}

impl ErrorKind for ReportingError {
	fn get_text(&self) -> String {
		match self {
			ReportingError::InvalidKismesisID => "Tried to report an error ocurring on a file with an invalid Kismesis ID.\nPlease contact the developer of the engine you're using.".into(),
		}
	}
}

impl<'a> DrawingInfo<'a> {
	pub fn from(scope: KisID, engine: &'a Kismesis, hint: bool) -> Result<Self, ()> {
		let scope = engine.get_file(scope).ok_or(())?;
		let lines: Vec<&[Token]> = scope
			.tokens
			.split_inclusive(|x| matches!(x, Token::Newline(_)))
			.collect();
		let lines = {
			let mut out = Vec::new();
			let mut len: usize = 0;
			for x in lines {
				out.push((len, x));
				len += x.len();
			}
			out
		};
		Ok(Self {
			line_number_length: 3,
			scope,
			lines,
			line_offset: (2, 2),
			hint,
		})
	}
}

pub fn draw_error<T: ErrorKind + Debug>(
	err: &ErrorState<T>,
	info: &Result<DrawingInfo, ()>,
	engine: &Kismesis,
) -> String {
	let info = match info.as_ref() {
		Ok(x) => x,
		Err(_) => {
			let err = ReportingError::InvalidKismesisID.stateless();
			return draw_stateless_error(&err, false, engine);
		}
	};
	let minimum_line = {
		let x = err.text_position.get_start_line();
		if x < info.line_offset.0 {
			0
		} else {
			x - info.line_offset.0
		}
	};
	let maximum_line = {
		let x = err.text_position.get_end_line();
		if x > info.lines.len() - info.line_offset.1 {
			info.lines.len()
		} else {
			x + info.line_offset.1
		}
	};

	let mut output = String::new();

	if info.hint {
		output.push_str(&" HINT ".black().on_yellow().to_string());
		output.push_str(&" in `".black().on_yellow().to_string());
		match info.scope.path {
			Some(ref path) => {
				output.push_str(
					&path
						.to_string_lossy()
						.to_string()
						.black()
						.on_yellow()
						.to_string(),
				);
				output.push_str(&"` ".black().on_yellow().to_string());
			}
			None => output.push_str(&"input` ".black().on_yellow().to_string()),
		}
	} else {
		output.push_str(&" ERROR ".black().on_red().to_string());
		output.push_str(&" in `".black().on_red().to_string());
		match info.scope.path {
			Some(ref path) => {
				output.push_str(
					&path
						.to_string_lossy()
						.to_string()
						.black()
						.on_red()
						.to_string(),
				);
				output.push_str(&"` ".black().on_red().to_string());
			}
			None => output.push_str(&"input` ".black().on_red().to_string()),
		}
	}
	output.push('\n');

	for line_number in minimum_line..=maximum_line {
		if let Some(string) = draw_line(line_number, err, info) {
			output.push_str(&string);
			output.push('\n');
		}
	}

	output.push('\n');

	for x in err.hints.iter() {
		let hint = match x {
			Hint::Stateful(x) => {
				draw_error(&x.error, &DrawingInfo::from(x.scope, engine, true), engine)
			}
			Hint::Stateless(x) => draw_stateless_error(x, true, engine),
		};
		output.push_str(&hint);
	}

	if !err.text_position.is_one_line() {
		output.push_str(&format!("\n{}", err.error.get_text()));
	}

	output
}

pub fn draw_stateless_error<T: ErrorKind + Debug>(
	err: &StatelessError<T>,
	hint: bool,
	engine: &Kismesis,
) -> String {
	let mut output = String::new();

	if hint {
		output.push_str(&" HINT ".black().on_yellow().to_string());
	} else {
		output.push_str(&" ERROR ".black().on_red().to_string());
	}
	output.push('\n');

	output.push_str(&format!("\n{}", err.error.get_text()));

	for x in err.hints.iter() {
		let hint = match x {
			Hint::Stateful(x) => {
				draw_error(&x.error, &DrawingInfo::from(x.scope, engine, true), engine)
			}
			Hint::Stateless(x) => draw_stateless_error(x, true, engine),
		};
		output.push_str(&hint);
	}

	output
}

fn draw_line<T: ErrorKind>(
	line_number: usize,
	err: &ErrorState<T>,
	info: &DrawingInfo,
) -> Option<String> {
	let mut output = draw_line_number(line_number, info).white().to_string();
	let mut error_line = turn_to_chars(draw_line_number(line_number, info), ' ');
	let termsize = termsize::get().map(|size| size.cols).unwrap_or(40) as usize;
	let termsize = std::cmp::min(termsize, termsize - err.error.get_text().len());
	if let Some(line) = info.lines.get(line_number) {
		let mut char_idx: usize = 0;
		for (token_idx, token) in line.1.iter().enumerate() {
			let token_pos = TokenPos::new_at(line.0 + token_idx, line_number, token_idx);
			let tkstr = match token {
				Token::Newline(_) if token_pos.is_in(&err.text_position) => "~".to_string(),
				Token::Newline(_) => "".to_string(),
				Token::Indent(_) => " ".repeat(4),
				x => x.get_as_string(),
			};
			char_idx += tkstr.len();
			if char_idx + tkstr.len() >= termsize && token_idx != 0 {
				if error_line.chars().any(|x| !x.is_whitespace()) {
					output.push('\n');
					output.push_str(error_line.yellow().to_string().trim_end());
					output.push('\n');
					output.push_str(&turn_to_chars(draw_line_number(line_number, info), ' '));
					error_line = turn_to_chars(draw_line_number(line_number, info), ' ');
				} else {
					output.push('\n');
					output.push_str(&turn_to_chars(draw_line_number(line_number, info), ' '));
					error_line = turn_to_chars(draw_line_number(line_number, info), ' ');
				}
				char_idx = tkstr.len();
			}
			output.push_str(&tkstr);
			let char = if token_pos.is_in(&err.text_position) {
				'^'
			} else {
				' '
			};
			error_line.push_str(&turn_to_chars(tkstr, char));
			if token_pos.is_at_an_end(&err.text_position) {
				if err.text_position.is_one_line() {
					error_line.push_str(&format!(" {}", err.error.get_text()));
				} else {
					error_line.push_str(" Error happened here");
				}
			}
		}
	} else {
		return None;
	}

	error_line = error_line.trim_end().to_string();
	if !error_line.is_empty() {
		Some(format!("{}\n{}", output, error_line.yellow()))
	} else {
		Some(output)
	}
}

fn turn_to_chars(string: String, chr: char) -> String {
	string
		.chars()
		.map(|x| match x {
			'\t' => chr.to_string().repeat(4),
			_ => chr.to_string(),
		})
		.collect()
}

fn draw_line_number(line: usize, info: &DrawingInfo) -> String {
	let mut output = (line + 1).to_string();
	while output.len() < info.line_number_length + 1 {
		output.push(' ');
	}
	output.push_str("│ ");
	output
}

pub fn draw_scoped_error<T: ErrorKind + Debug>(err: &ScopedError<T>, engine: &Kismesis) -> String {
	draw_error(
		&err.error,
		&DrawingInfo::from(err.scope, engine, false),
		engine,
	)
}
