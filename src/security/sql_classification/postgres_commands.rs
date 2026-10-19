use sqlparser::dialect::PostgreSqlDialect;
use sqlparser::keywords::Keyword;
use sqlparser::parser::{Parser, ParserError};
use sqlparser::tokenizer::Token;

type ParseResult = std::result::Result<(), ParserError>;

// Recognize only complete outer command forms missing from sqlparser. Other
// unsupported forms remain recoverable rejections and never execute.
pub(super) fn recognizes(tokens: &[Token]) -> bool {
    let Some(Token::Word(command)) = tokens.first() else {
        return false;
    };
    if command.quote_style.is_some() {
        return false;
    }
    let mut parser = Parser::new(&PostgreSqlDialect {}).with_tokens(tokens[1..].to_vec());
    let result = match command.value.to_ascii_uppercase().as_str() {
        "CHECKPOINT" => Ok(()),
        "REFRESH" => refresh(&mut parser),
        "CLUSTER" => cluster(&mut parser),
        "REASSIGN" => reassign(&mut parser),
        "IMPORT" => import(&mut parser),
        "SECURITY" => security_label(&mut parser),
        "DO" => do_block(&mut parser),
        "REINDEX" => reindex(&mut parser),
        "ANALYZE" | "ANALYSE" | "VACUUM" if tokens.len() == 1 => Ok(()),
        _ => return false,
    };
    result.is_ok() && parser.next_token().token == Token::EOF
}

fn refresh(parser: &mut Parser<'_>) -> ParseResult {
    parser.expect_keywords(&[Keyword::MATERIALIZED, Keyword::VIEW])?;
    let _ = parser.parse_keyword(Keyword::CONCURRENTLY);
    parser.parse_object_name(false)?;
    if parser.parse_keyword(Keyword::WITH) {
        let _ = parser.parse_keyword(Keyword::NO);
        parser.expect_keyword_is(Keyword::DATA)?;
    }
    Ok(())
}

fn cluster(parser: &mut Parser<'_>) -> ParseResult {
    let _ = parser.parse_keyword(Keyword::VERBOSE);
    if parser.peek_token().token != Token::EOF {
        parser.parse_object_name(false)?;
        if parser.parse_keyword(Keyword::USING) {
            parser.parse_identifier()?;
        }
    }
    Ok(())
}

fn reassign(parser: &mut Parser<'_>) -> ParseResult {
    parser.expect_keywords(&[Keyword::OWNED, Keyword::BY])?;
    parser.parse_comma_separated(Parser::parse_identifier)?;
    parser.expect_keyword_is(Keyword::TO)?;
    parser.parse_identifier()?;
    Ok(())
}

fn import(parser: &mut Parser<'_>) -> ParseResult {
    parser.expect_keywords(&[Keyword::FOREIGN, Keyword::SCHEMA])?;
    parser.parse_identifier()?;
    let has_filter = if parser.parse_keyword(Keyword::LIMIT) {
        parser.expect_keyword_is(Keyword::TO)?;
        true
    } else {
        parser.parse_keyword(Keyword::EXCEPT)
    };
    if has_filter {
        parser.expect_token(&Token::LParen)?;
        parser.parse_comma_separated(Parser::parse_identifier)?;
        parser.expect_token(&Token::RParen)?;
    }
    parser.expect_keywords(&[Keyword::FROM, Keyword::SERVER])?;
    parser.parse_identifier()?;
    parser.expect_keyword_is(Keyword::INTO)?;
    parser.parse_identifier()?;
    Ok(())
}

fn security_label(parser: &mut Parser<'_>) -> ParseResult {
    expect_word(parser, &["LABEL"])?;
    if parser.parse_keyword(Keyword::FOR) {
        parser.parse_identifier()?;
    }
    parser.expect_keyword_is(Keyword::ON)?;
    expect_word(
        parser,
        &[
            "TABLE",
            "COLUMN",
            "DATABASE",
            "DOMAIN",
            "LANGUAGE",
            "PUBLICATION",
            "ROLE",
            "SCHEMA",
            "SEQUENCE",
            "SUBSCRIPTION",
            "TABLESPACE",
            "TYPE",
            "VIEW",
        ],
    )?;
    parser.parse_object_name(false)?;
    parser.expect_keyword_is(Keyword::IS)?;
    if !parser.parse_keyword(Keyword::NULL) {
        expect_string(parser)?;
    }
    Ok(())
}

fn do_block(parser: &mut Parser<'_>) -> ParseResult {
    let language_first = parser.parse_keyword(Keyword::LANGUAGE);
    if language_first {
        parser.parse_identifier()?;
    }
    expect_string(parser)?;
    if !language_first && parser.parse_keyword(Keyword::LANGUAGE) {
        parser.parse_identifier()?;
    }
    Ok(())
}

fn reindex(parser: &mut Parser<'_>) -> ParseResult {
    parser.expect_one_of_keywords(&[
        Keyword::INDEX,
        Keyword::TABLE,
        Keyword::SCHEMA,
        Keyword::DATABASE,
        Keyword::SYSTEM,
    ])?;
    let _ = parser.parse_keyword(Keyword::CONCURRENTLY);
    parser.parse_object_name(false)?;
    Ok(())
}

fn expect_word(parser: &mut Parser<'_>, expected: &[&str]) -> ParseResult {
    if matches!(parser.next_token().token, Token::Word(word) if word.quote_style.is_none() && expected.contains(&word.value.to_ascii_uppercase().as_str()))
    {
        Ok(())
    } else {
        Err(ParserError::ParserError(
            "Unsupported command syntax".into(),
        ))
    }
}

fn expect_string(parser: &mut Parser<'_>) -> ParseResult {
    match parser.next_token().token {
        Token::SingleQuotedString(_)
        | Token::DollarQuotedString(_)
        | Token::EscapedStringLiteral(_)
        | Token::UnicodeStringLiteral(_) => Ok(()),
        _ => Err(ParserError::ParserError("Expected a string literal".into())),
    }
}
