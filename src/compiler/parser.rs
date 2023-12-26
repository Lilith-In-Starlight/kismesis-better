pub(crate) mod errors;
pub(crate) mod state;
pub(crate) mod types;

use std::fmt::Debug;
use std::path::PathBuf;

use crate::compiler::lexer::Token;

use self::errors::{Err, ParseError};
use self::state::ParserState;
use self::types::{
    Argument, Attribute, BinFunc, BodyNodes, BodyTags, Expression, HtmlNodes, HtmlTag, Lambda,
    Macro, ParsedFile, PlugCall, Ranged, StringParts, Tag, TopNodes, UniFunc, Variable,
};

type ParserResult<'a, T> = Result<(T, ParserState<'a>), Err>;

pub(crate) trait Parser<'a, Output> {
    fn parse(&self, state: ParserState<'a>) -> ParserResult<'a, Output>;

    fn map<F, T2>(self, fun: F) -> BoxedParser<'a, T2>
    where
        Self: Sized + 'a,
        F: Fn(Output) -> T2 + 'a,
        T2: 'a,
        Output: 'a,
    {
        BoxedParser::new(map(self, fun))
    }

    fn dbg(self) -> BoxedParser<'a, Output>
    where
        Self: Sized + 'a,
        Output: Debug + 'a,
    {
        BoxedParser::new(dbg(self))
    }
    fn or<P>(self, other: P) -> BoxedParser<'a, Output>
    where
        Self: Sized + 'a,
        P: Parser<'a, Output> + 'a,
        Output: 'a,
    {
        BoxedParser::new(or(self, other))
    }

    fn followed_by<P, O2>(self, other: P) -> BoxedParser<'a, Output>
    where
        Self: Sized + 'a,
        P: Parser<'a, O2> + 'a,
        Output: 'a,
        O2: 'a,
    {
        BoxedParser::new(followed_by(self, other))
    }
    fn preceding<P, O2>(self, other: P) -> BoxedParser<'a, O2>
    where
        Self: Sized + 'a,
        P: Parser<'a, O2> + 'a,
        Output: 'a,
        O2: 'a,
    {
        BoxedParser::new(preceding(self, other))
    }
    fn and_also<P, O2>(self, other: P) -> BoxedParser<'a, (Output, O2)>
    where
        Self: Sized + 'a,
        P: Parser<'a, O2> + 'a,
        Output: 'a,
        O2: 'a,
    {
        BoxedParser::new(and_also(self, other))
    }
    fn and_maybe<P, O2>(self, other: P) -> BoxedParser<'a, (Output, Option<O2>)>
    where
        Self: Sized + 'a,
        P: Parser<'a, O2> + 'a,
        Output: 'a,
        O2: 'a,
    {
        BoxedParser::new(and_maybe(self, other))
    }
}

impl<'a, Output, F> Parser<'a, Output> for F
where
    F: Fn(ParserState<'a>) -> ParserResult<'a, Output>,
{
    fn parse(&self, state: ParserState<'a>) -> ParserResult<'a, Output> {
        self(state)
    }
}

pub(crate) struct BoxedParser<'a, T> {
    parser: Box<dyn Parser<'a, T> + 'a>,
}

impl<'a, T> BoxedParser<'a, T> {
    fn new<P>(parser: P) -> Self
    where
        P: Parser<'a, T> + 'a,
    {
        Self {
            parser: Box::new(parser),
        }
    }
}

impl<'a, T> Parser<'a, T> for BoxedParser<'a, T> {
    fn parse(&self, state: ParserState<'a>) -> ParserResult<'a, T> {
        self.parser.parse(state)
    }
}

// Parsers
fn quote_mark(state: ParserState) -> ParserResult<&char> {
    match character('\'').or(character('"')).parse(state.clone()) {
        Err(_) => Err(ParseError::NotQuoteMark.state_at(&state)),
        ok => ok,
    }
}

fn tag_opener(state: ParserState) -> ParserResult<&char> {
    match character('<').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedTagOpener.state_at(&state)),
        ok => ok,
    }
}

fn subtag_opener(state: ParserState) -> ParserResult<&char> {
    match character('+').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedTagOpener.state_at(&state)),
        ok => ok,
    }
}

fn tag_closer(state: ParserState) -> ParserResult<&char> {
    match character('>').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedTagCloser.state_at(&state)),
        ok => ok,
    }
}

fn expr_opener(state: ParserState) -> ParserResult<&char> {
    match character('{').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedExprStart.state_at(&state)),
        ok => ok,
    }
}

fn expr_closer(state: ParserState) -> ParserResult<&char> {
    match character('}').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedExprEnd.state_at(&state)),
        ok => ok,
    }
}

fn macro_mark(state: ParserState) -> ParserResult<&char> {
    match character('!').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedMacroMark.state_at(&state)),
        ok => ok,
    }
}

fn plugin_mark(state: ParserState) -> ParserResult<&char> {
    match character('?').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedPluginMark.state_at(&state)),
        ok => ok,
    }
}

fn body_opener(state: ParserState) -> ParserResult<&char> {
    match character('|').or(newline).parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedBodyOpener.state_at(&state)),
        ok => ok,
    }
}

fn macro_name(state: ParserState) -> ParserResult<&str> {
    match literal.parse(state.clone()) {
        Ok(ok) if ok.0 != "content" => Ok(ok),
        _ => Err(ParseError::ExpectedTagName.state_at(&state)),
    }
}

fn equals(state: ParserState) -> ParserResult<&char> {
    match character('=').parse(state.clone()) {
        Err(_) => Err(ParseError::ExpectedEquals.state_at(&state)),
        ok => ok,
    }
}

fn variable_name(state: ParserState) -> ParserResult<&str> {
    literal
        .parse(state.clone())
        .map_err(|_x| ParseError::ExpectedVarName.state_at(&state))
}

fn expression(state: ParserState) -> ParserResult<Expression> {
    let parser = variable_name
        .map(|x| Expression::Variable(x.to_owned()))
        .or(quoted.map(Expression::Literal))
        .or(wrapped_expr);
    parser.parse(state)
}

fn binary_func(state: ParserState) -> ParserResult<BinFunc> {
    let (val, next_state) = literal
        .parse(state.clone())
        .map_err(|_x| ParseError::ExpectedBinFunc.state_at(&state))?;
    match val {
        "and" => Ok((BinFunc::And, next_state)),
        "or" => Ok((BinFunc::Or, next_state)),
        _ => Err(ParseError::ExpectedBinFunc.state_at(&state)),
    }
}

fn unary_func(state: ParserState) -> ParserResult<UniFunc> {
    let (val, next_state) = literal
        .parse(state.clone())
        .map_err(|_x| ParseError::ExpectedUniFunc.state_at(&state))?;
    match val {
        "not" => Ok((UniFunc::Not, next_state)),
        _ => Err(ParseError::ExpectedUniFunc.state_at(&state)),
    }
}

fn binary_func_expr(state: ParserState) -> ParserResult<Expression> {
    let parser = get_range(expression)
        .and_also(after_spaces(binary_func))
        .and_also(cut(after_spaces(get_range(expression))));
    let (((expr1, fun), expr2), next_state) = parser.parse(state)?;
    Ok((
        Expression::BinFunc(fun, Box::new(expr1), Box::new(expr2)),
        next_state,
    ))
}

fn unary_func_expr(state: ParserState) -> ParserResult<Expression> {
    let parser = unary_func.and_also(cut(after_spaces(get_range(expression))));
    let ((fun, expr), next_state) = parser.parse(state)?;
    Ok((Expression::UniFunc(fun, Box::new(expr)), next_state))
}

fn wrapped_expr(state: ParserState) -> ParserResult<Expression> {
    let internal_parser = binary_func_expr
        .or(unary_func_expr)
        .or(expression)
        .or(character('!').map(|_x| Expression::None));
    let parser = expr_opener.preceding(cut(after_spaces(internal_parser)).followed_by(expr_closer));

    parser.parse(state)
}

fn variable_definition(state: ParserState) -> ParserResult<Variable> {
    let parser = var_def_starter
        .preceding(after_spaces(literal))
        .and_also(cut(after_spaces(equals).preceding(after_spaces(quoted))));
    let ((name, value), next_state) = parser.parse(state)?;
    Ok((
        Variable {
            name: name.to_owned(),
            value,
        },
        next_state,
    ))
}

fn lambda_definition(state: ParserState) -> ParserResult<Lambda> {
    let parser =
        lambda_def_starter.preceding(cut(after_spaces(literal)).and_maybe(after_spaces(equals).preceding(after_spaces(quoted))));
    let ((name, value), next_state) = parser.parse(state)?;
    Ok((
        Lambda {
            name: name.to_owned(),
            value,
        },
        next_state,
    ))
}

fn quoted(state: ParserState) -> ParserResult<Vec<StringParts>> {
    let (opener, mut state) = quote_mark.parse(state)?;
    let mut output = Vec::<StringParts>::new();
    let mut escape = false;
    while let Some(token) = state.first_token() {
        match token {
            Token::Symbol(sym) if *sym == '@' && !escape => {
                state = state.next_state();
                let (val, next_state) = get_range(expression).parse(state)?;
                output.push(StringParts::Expression(val));
                state = next_state;
            }
            Token::Symbol(sym) if sym == opener && !escape => {
                return Ok((output, state.next_state()))
            }
            Token::Symbol(sym) if *sym == '\\' && !escape => {
                escape = true;
                state = state.next_state();
            }
            Token::Newline(_) => return Err(ParseError::NewlineInQuote.state_at(&state)),
            tok => match output.pop() {
                Some(StringParts::String(mut string)) => {
                    tok.push_to_string(&mut string);
                    output.push(StringParts::String(string));
                    state = state.next_state();
                }
                Some(StringParts::Expression(var)) => {
                    output.push(StringParts::Expression(var));
                    output.push(StringParts::String(tok.get_as_string()));
                    state = state.next_state();
                }
                None => {
                    output.push(StringParts::String(tok.get_as_string()));
                    state = state.next_state();
                }
            },
        }
    }

    Err(ParseError::UnclosedQuote.state_at(&state).cut())
}

fn some_tag(state: ParserState) -> ParserResult<Tag> {
    let parser = tag_opener.preceding(cut(after_spaces(
        tag.map(Tag::HtmlTag)
            .or(macro_call.map(Tag::MacroCall))
            .or(macro_def.map(Tag::MacroDef))
            .or(plug_call.map(Tag::PlugCall))
            .or(content_macro.map(|_| Tag::Content))
            .followed_by(skipped_blanks().preceding(tag_closer)),
    )));

    parser.parse(state)
}

fn some_child_tag(state: ParserState) -> ParserResult<BodyTags> {
    let parser = character('<').preceding(cut(after_spaces(
        tag.map(BodyTags::HtmlTag)
            .or(macro_call.map(BodyTags::MacroCall))
            .or(content_macro.map(|_| BodyTags::Content))
            .followed_by(skipped_blanks().preceding(tag_closer)),
    )));

    parser.parse(state)
}

fn tag(state: ParserState<'_>) -> ParserResult<'_, HtmlTag> {
    let parser = tag_head.and_maybe(tag_body);

    let (((name, attributes, subtags), body), state) = parser.parse(state)?;
    Ok((
        HtmlTag {
            name,
            attributes,
            body: body.unwrap_or(vec![]),
            subtags,
        },
        state,
    ))
}

fn content_macro(state: ParserState<'_>) -> ParserResult<'_, ()> {
    let parser = specific_literal("content").followed_by(after_spaces(macro_mark));
    let (_, state) = parser.parse(state)?;
    Ok(((), state))
}

fn plug_call(state: ParserState<'_>) -> ParserResult<'_, Box<PlugCall>> {
    let parser = plugin_head.and_maybe(plugin_body);

    let (((name, arguments), body), state) = parser.parse(state)?;
    Ok((
        Box::new(PlugCall {
            name,
            arguments,
            body,
        }),
        state,
    ))
}

fn macro_call(state: ParserState<'_>) -> ParserResult<'_, Macro> {
    let parser = macro_call_head;

    let ((name, arguments), state) = parser.parse(state)?;
    Ok((
        Macro {
            name,
            arguments,
            body: vec![],
        },
        state,
    ))
}

fn macro_def(state: ParserState<'_>) -> ParserResult<'_, Macro> {
    let parser = macro_def_head.and_maybe(tag_body);

    let (((name, arguments), body), state) = parser.parse(state)?;
    Ok((
        Macro {
            name,
            arguments,
            body: body.unwrap_or(vec![]),
        },
        state,
    ))
}

fn space(state: ParserState) -> ParserResult<&char> {
    match state.advanced() {
        (Some(Token::Space(space)), next_state) => Ok((space, next_state)),
        (_, next_state) => Err(ParseError::NotASpace.state_at(&next_state)),
    }
}

fn indent(state: ParserState) -> ParserResult<&char> {
    match state.advanced() {
        (Some(Token::Indent(indent)), next_state) => Ok((indent, next_state)),
        (_, next_state) => Err(ParseError::NotAnIndent.state_at(&next_state)),
    }
}

fn newline(state: ParserState) -> ParserResult<&char> {
    match state.advanced() {
        (Some(Token::Newline(newline)), next_state) => ParserResult::Ok((newline, next_state)),
        (_, next_state) => Err(ParseError::NotANewline.state_at(&next_state)),
    }
}

fn some_symbol(state: ParserState) -> ParserResult<&char> {
    match state.advanced() {
        (Some(Token::Symbol(x)), next_state) => Ok((x, next_state)),
        _ => Err(ParseError::NotSymbol.state_at(&state)),
    }
}
fn literal(state: ParserState) -> ParserResult<&str> {
    match state.advanced() {
        (Some(Token::Word(x)), next_state) => Ok((x, next_state)),
        _ => Err(ParseError::NotLiteral.state_at(&state)),
    }
}

fn non_macro_starter(state: ParserState) -> ParserResult<&str> {
    match state.advanced() {
        (Some(Token::Word(x)), next_state) if x != "macro" => Ok((x, next_state)),
        (Some(Token::Word(x)), _) if x == "macro" => {
            Err(ParseError::UnexpectedMacroDef.state_at(&state))
        }
        _ => Err(ParseError::ExpectedTagName.state_at(&state)),
    }
}

fn var_def_starter(state: ParserState) -> ParserResult<&str> {
    match state.advanced() {
        (Some(Token::Word(x)), next_state) if x == "let" => Ok((x, next_state)),
        _ => Err(ParseError::NotLiteral.state_at(&state)),
    }
}

fn lambda_def_starter(state: ParserState) -> ParserResult<&str> {
    match state.advanced() {
        (Some(Token::Word(x)), next_state) if x == "lambda" => Ok((x, next_state)),
        _ => Err(ParseError::NotLiteral.state_at(&state)),
    }
}

fn macro_starter(state: ParserState) -> ParserResult<&str> {
    match state.advanced() {
        (Some(Token::Word(x)), next_state) if x == "macro" => Ok((x, next_state)),
        (Some(Token::Word(x)), _) if x != "macro" => {
            Err(ParseError::ExpectedTagNameOrMacroDef.state_at(&state))
        }
        _ => Err(ParseError::NotMacroStart.state_at(&state)),
    }
}

fn tag_head(state: ParserState) -> ParserResult<(Ranged<String>, Vec<Attribute>, Vec<HtmlTag>)> {
    let cut_cond = space
        .or(indent)
        .or(body_opener)
        .or(tag_closer)
        .or(tag_opener)
        .or(subtag_opener);
    let parser = get_range(non_macro_starter)
        .followed_by(peek(cut_cond))
        .and_also(cut(zero_or_more(after_spaces(attribute))))
        .and_also(zero_or_more(after_spaces(subtag)));

    let (((name, attributes), subtags), state) = parser.parse(state)?;

    Ok(((name.to_own(), attributes, subtags), state))
}

fn plugin_head(state: ParserState) -> ParserResult<(Ranged<String>, Ranged<Vec<Token>>)> {
    let parser = get_range(non_macro_starter)
        .followed_by(plugin_mark)
        .followed_by(skip_spaces());

    let (name, mut state) = parser.parse(state)?;
    let start = state.position;
    let mut tokens = Vec::new();
    let mut escape = false;

    while let Some(token) = state.first_token() {
        match token {
            Token::Symbol(symbol) if symbol == &'\\' && !escape => {
                escape = true;
                state = state.next_state();
            }
            Token::Symbol(x) if !escape && (x == &'>' || x == &'|') => {
                let end = state.position;
                return Ok((
                    (
                        name.to_own(),
                        Ranged {
                            value: tokens,
                            range: (start, end),
                        },
                    ),
                    state,
                ));
            }
            Token::Newline(_) => {
                let end = state.position;
                return Ok((
                    (
                        name.to_own(),
                        Ranged {
                            value: tokens,
                            range: (start, end),
                        },
                    ),
                    state,
                ));
            }
            tok => {
                tokens.push(tok.clone());
                state = state.next_state();
            }
        }
    }

    Err(ParseError::EndlessString.state_at(&state).cut())
}

fn macro_call_head(state: ParserState) -> ParserResult<(Ranged<String>, Vec<Argument>)> {
    let parser = get_range(macro_name)
        .followed_by(macro_mark)
        .and_also(cut(zero_or_more(skip_spaces().preceding(argument))));

    let ((name, attributes), state) = parser.parse(state)?;

    Ok(((name.to_own(), attributes), state))
}

fn macro_def_head(state: ParserState) -> ParserResult<(Ranged<String>, Vec<Argument>)> {
    let parser = after_spaces(macro_starter).preceding(
        cut(after_spaces(get_range(literal))).and_also(zero_or_more(after_spaces(argument))),
    );

    let ((name, attributes), state) = parser.parse(state)?;

    Ok(((name.to_own(), attributes), state))
}

fn skip_spaces<'a>() -> impl Parser<'a, Vec<&'a char>> {
    zero_or_more(space.or(indent))
}

fn after_spaces<'a, T1, P>(parser: P) -> impl Parser<'a, T1>
where
    P: Parser<'a, T1> + 'a,
    T1: 'a,
{
    skip_spaces().preceding(parser)
}

fn skipped_blanks<'a>() -> impl Parser<'a, Vec<&'a char>> {
    zero_or_more(space.or(indent).or(newline))
}

fn skip_newline_blanks<'a>() -> impl Parser<'a, Vec<&'a char>> {
    zero_or_more(skip_spaces().preceding(newline))
}

fn tag_body(state: ParserState) -> ParserResult<Vec<HtmlNodes>> {
    let parser = skip_spaces().preceding(body_opener).preceding(skipped_blanks()).preceding(zero_or_more(
        skip_newline_blanks().preceding(string)
            .map(HtmlNodes::String)
            .or(skipped_blanks().preceding(some_child_tag.map(|x| x.into()))),
    ));

    parser.parse(state)
}

fn plugin_body(state: ParserState) -> ParserResult<Ranged<Vec<Token>>> {
    let parser = skip_spaces()
        .preceding(body_opener)
        .followed_by(skipped_blanks());
    let (_, mut state) = parser.parse(state)?;

    let start = state.position;
    let mut tokens = Vec::new();
    let mut escape = false;

    while let Some(token) = state.first_token() {
        match token {
            Token::Symbol(symbol) if symbol == &'\\' && !escape => {
                escape = true;
                state = state.next_state();
            }
            Token::Symbol(x) if !escape && (x == &'>') => {
                let end = state.position;
                return Ok((
                    Ranged {
                        value: tokens,
                        range: (start, end),
                    },
                    state,
                ));
            }
            tok => {
                tokens.push(tok.clone());
                state = state.next_state();
            }
        }
    }

    Err(ParseError::EndlessString.state_at(&state).cut())
}
fn string(mut state: ParserState) -> ParserResult<Vec<StringParts>> {
    let mut output = Vec::<StringParts>::new();
    let mut escape = false;
    let mut in_newline = false;
    while let Some(token) = state.first_token() {
        match token {
            Token::Symbol(sym) if *sym == '@' && !escape => {
                state = state.next_state();
                let (val, next_state) = get_range(expression).parse(state)?;
                output.push(StringParts::Expression(val));
                state = next_state;
            }
            Token::Symbol(sym) if *sym == '<' && !escape => {
                if !output.is_empty() {
                    return Ok((output, state));
                } else {
                    return Err(ParseError::EmptyString.state_at(&state));
                }
            }
            Token::Symbol(sym) if *sym == '>' && !escape => {
                if !output.is_empty() {
                    return Ok((output, state));
                } else {
                    return Err(ParseError::EmptyString.state_at(&state));
                }
            }
            Token::Symbol(sym) if *sym == '\\' && !escape => {
                escape = true;
                state = state.next_state();
            }
            Token::Newline(_) => {
                in_newline = true;
                state = state.next_state();
            }
            Token::Space(_) | Token::Indent(_) if in_newline && !escape => {
                state = state.next_state();
            }
            tok => match output.pop() {
                Some(StringParts::String(mut string)) => {
                    tok.push_to_string(&mut string);
                    output.push(StringParts::String(string));
                    state = state.next_state();
                }
                Some(StringParts::Expression(var)) => {
                    output.push(StringParts::Expression(var));
                    output.push(StringParts::String(tok.get_as_string()));
                    state = state.next_state();
                }
                None => {
                    output.push(StringParts::String(tok.get_as_string()));
                    state = state.next_state();
                }
            },
        }
    }

    Err(ParseError::EndlessString.state_at(&state).cut())
}

fn subtag(state: ParserState) -> ParserResult<HtmlTag> {
    let parser = subtag_opener.preceding(
        cut(after_spaces(get_range(literal)))
            .and_also(zero_or_more(skip_spaces().preceding(attribute))),
    );
    let ((name, attributes), state) = parser.parse(state)?;
    Ok((
        HtmlTag {
            name: name.to_own(),
            attributes,
            subtags: vec![],
            body: vec![],
        },
        state,
    ))
}

fn attribute(state: ParserState) -> ParserResult<Attribute> {
    let parser = get_range(literal).followed_by(skip_spaces()).and_also(cut(
        equals.preceding(zero_or_more(space.or(indent)).preceding(quoted))
    ));
    let ((name, value), state) = parser.parse(state)?;
    Ok((
        Attribute {
            name: name.to_own(),
            value,
        },
        state,
    ))
}

fn argument(state: ParserState) -> ParserResult<Argument> {
    let parser = get_range(literal)
        .followed_by(zero_or_more(space.or(indent)))
        .and_maybe(equals.preceding(zero_or_more(space.or(indent)).preceding(quoted)));
    let ((name, value), state) = parser.parse(state)?;
    Ok((
        Argument {
            name: name.to_own(),
            value,
        },
        state,
    ))
}

pub fn file<'a>(tokens: Vec<Token>, path: Option<PathBuf>) -> Result<ParsedFile<'a>, (Err, Vec<Token>)> {
    let parser = zero_or_more(
        skipped_blanks().preceding(
            some_tag
                .map(|x| x.into())
                .or(lambda_definition.map(BodyNodes::LambdaDef))
                .or(variable_definition.map(BodyNodes::VarDef)),
        ),
    );

    let state = ParserState::new(&tokens);
    let ast_nodes = match parser.parse(state) {
        Ok((val, _)) => val,
        Err(err) => {
            drop(parser);
            return Err((err, tokens))
        },
    };
    drop(parser);
    let mut output = ParsedFile::new(tokens, path);
    for node in ast_nodes {
        match node {
            BodyNodes::HtmlTag(tag) => output.body.push(TopNodes::HtmlTag(tag.merge_subtags())),
            BodyNodes::MacroDef(mac) => output.defined_macros.push(mac),
            BodyNodes::MacroCall(mac) => output.body.push(TopNodes::MacroCall(mac)),
            BodyNodes::String(_string) => todo!("Markup syntax"),
            BodyNodes::LambdaDef(lambda) => output.defined_lambdas.push(lambda),
            BodyNodes::VarDef(var) => output.defined_variables.push(var),
            BodyNodes::PlugCall(plug) => output.body.push(TopNodes::PlugCall(plug)),
            BodyNodes::Content => output.body.push(TopNodes::Content),
        }
    }

    Ok(output)
}
// Generators

fn preceding<'a, P1, O1, P2, O2>(p1: P1, p2: P2) -> impl Parser<'a, O2>
where
    P1: Parser<'a, O1>,
    P2: Parser<'a, O2>,
{
    move |state| match p1.parse(state) {
        Ok((_, next_state)) => Ok(p2.parse(next_state)?),
        Err(error) => Err(error),
    }
}

fn and_also<'a, P1, O1, P2, O2>(p1: P1, p2: P2) -> impl Parser<'a, (O1, O2)>
where
    P1: Parser<'a, O1>,
    P2: Parser<'a, O2>,
{
    move |state| match p1.parse(state) {
        Ok((first_result, next_state)) => match p2.parse(next_state) {
            Ok((second_result, next_state)) => Ok(((first_result, second_result), next_state)),
            Err(err) => Err(err),
        },
        Err(error) => Err(error),
    }
}
fn and_maybe<'a, P1, O1, P2, O2>(p1: P1, p2: P2) -> impl Parser<'a, (O1, Option<O2>)>
where
    P1: Parser<'a, O1>,
    P2: Parser<'a, O2>,
{
    move |state| match p1.parse(state) {
        ParserResult::Ok((first_result, next_state)) => match p2.parse(next_state.clone()) {
            Ok((second_result, next_state)) => {
                Ok(((first_result, Some(second_result)), next_state))
            }
            Err(Err::Error(_)) => Ok(((first_result, None), next_state)),
            Err(x) => Err(x),
        },
        Err(error) => Err(error),
    }
}
fn followed_by<'a, P1, O1, P2, O2>(p1: P1, p2: P2) -> impl Parser<'a, O1>
where
    P1: Parser<'a, O1>,
    P2: Parser<'a, O2>,
{
    move |state| match p1.parse(state) {
        Ok((result, next_state)) => match p2.parse(next_state) {
            Ok((_, next_state)) => Ok((result, next_state)),
            Err(err) => Err(err),
        },
        Err(error) => Err(error),
    }
}

fn or<'a, P1, O1, P2>(p1: P1, p2: P2) -> impl Parser<'a, O1>
where
    P1: Parser<'a, O1>,
    P2: Parser<'a, O1>,
{
    move |state: ParserState<'a>| match p1.parse(state.clone()) {
        Ok((result, next_state)) => Ok((result, next_state)),
        Err(Err::Failure(x)) => Err(Err::Failure(x)),
        Err(_) => p2.parse(state),
    }
}

fn character<'a>(chr: char) -> impl Parser<'a, &'a char> {
    move |state: ParserState<'a>| match some_symbol.parse(state.clone()) {
        Ok((x, next_state)) if x == &chr => Ok((x, next_state)),
        Ok((x, _)) => Err(ParseError::CharacterNotMatch {
            expected: chr,
            got: Some(*x),
        }
        .state_at(&state)),
        Err(error) => Err(error),
    }
}
fn specific_literal<'a>(word: &'a str) -> impl Parser<'a, &'a str> {
    move |state: ParserState<'a>| match literal.parse(state.clone()) {
        Ok((x, next_state)) if x == word => Ok((x, next_state)),
        Ok((x, _)) => Err(ParseError::LiteralNotMatch {
            expected: word.to_string(),
            got: Some(x.to_string()),
        }
        .state_at(&state)),
        Err(error) => Err(error),
    }
}

fn zero_or_more<'a, P, T>(parser: P) -> impl Parser<'a, Vec<T>>
where
    P: Parser<'a, T>,
{
    move |state: ParserState<'a>| {
        let mut state = state;
        let mut found = Vec::<T>::new();
        loop {
            match parser.parse(state.clone()) {
                Ok((token, next_state)) => {
                    state = next_state;
                    found.push(token);
                }
                Err(Err::Failure(x)) => return Err(Err::Failure(x)),
                _ => break,
            }
        }
        Ok((found, state))
    }
}

fn peek<'a, P, T>(parser: P) -> impl Parser<'a, T>
where
    P: Parser<'a, T>,
{
    move |state: ParserState<'a>| {
        let (val, _) = parser.parse(state.clone())?;
        Ok((val, state))
    }
}

fn dbg<'a, P, T: Debug>(parser: P) -> impl Parser<'a, T>
where
    P: Parser<'a, T>,
{
    move |state: ParserState<'a>| {
        let r = parser.parse(state);
        println!("{:#?}", r);
        r
    }
}

fn cut<'a, P, T>(parser: P) -> impl Parser<'a, T>
where
    P: Parser<'a, T>,
{
    move |state: ParserState<'a>| match parser.parse(state) {
        Err(Err::Error(x)) => Err(Err::Failure(x)),
        pat => pat,
    }
}

fn map<'a, P, F, T1, T2>(parser: P, fun: F) -> impl Parser<'a, T2>
where
    P: Parser<'a, T1>,
    F: Fn(T1) -> T2,
{
    move |state: ParserState<'a>| parser.parse(state).map(|(val, state)| (fun(val), state))
}

fn get_range<'a, P, T1>(parser: P) -> impl Parser<'a, Ranged<T1>>
where
    P: Parser<'a, T1>,
{
    move |state: ParserState<'a>| {
        let start = state.position;
        let (val, next_state) = parser.parse(state)?;
        let end = next_state.position;
        Ok((
            Ranged {
                value: val,
                range: (start, end),
            },
            next_state,
        ))
    }
}
