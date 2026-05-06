//! # SQL Lexer (Tokenizer)

/// Token types.
#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Select, From, Where, Insert, Into, Values, Update, Set,
    Delete, Create, Drop, Table, Index, On, And, Or, Not,
    Null, Is, As, Order, By, Asc, Desc, Limit, Primary, Key,
    Int, Integer, Bigint, Float, Double, Varchar, Text, Blob,
    Boolean, True, False, Unique,
    Identifier(String),
    StringLiteral(String),
    IntegerLiteral(i64),
    FloatLiteral(f64),
    Star, Comma, Semicolon, LeftParen, RightParen,
    Eq, NotEq, Lt, LtEq, Gt, GtEq,
    Plus, Minus, Slash, Dot,
    Eof,
}

/// Tokenize a SQL string.
pub fn tokenize(input: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut pos = 0;

    while pos < chars.len() {
        let ch = chars[pos];
        if ch.is_whitespace() { pos += 1; continue; }

        // Comments
        if ch == '-' && pos + 1 < chars.len() && chars[pos + 1] == '-' {
            while pos < chars.len() && chars[pos] != '\n' { pos += 1; }
            continue;
        }

        // Single-char symbols
        match ch {
            '*' => { tokens.push(Token::Star); pos += 1; continue; }
            ',' => { tokens.push(Token::Comma); pos += 1; continue; }
            ';' => { tokens.push(Token::Semicolon); pos += 1; continue; }
            '(' => { tokens.push(Token::LeftParen); pos += 1; continue; }
            ')' => { tokens.push(Token::RightParen); pos += 1; continue; }
            '+' => { tokens.push(Token::Plus); pos += 1; continue; }
            '/' => { tokens.push(Token::Slash); pos += 1; continue; }
            '.' => { tokens.push(Token::Dot); pos += 1; continue; }
            '=' => { tokens.push(Token::Eq); pos += 1; continue; }
            '-' => { tokens.push(Token::Minus); pos += 1; continue; }
            _ => {}
        }

        // Multi-char operators
        if ch == '<' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::LtEq); pos += 2;
            } else if pos + 1 < chars.len() && chars[pos + 1] == '>' {
                tokens.push(Token::NotEq); pos += 2;
            } else {
                tokens.push(Token::Lt); pos += 1;
            }
            continue;
        }
        if ch == '>' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::GtEq); pos += 2;
            } else {
                tokens.push(Token::Gt); pos += 1;
            }
            continue;
        }
        if ch == '!' {
            if pos + 1 < chars.len() && chars[pos + 1] == '=' {
                tokens.push(Token::NotEq); pos += 2; continue;
            }
            return Err(format!("unexpected '!' at {}", pos));
        }

        // String literals
        if ch == '\'' {
            pos += 1;
            let mut s = String::new();
            while pos < chars.len() && chars[pos] != '\'' {
                s.push(chars[pos]); pos += 1;
            }
            if pos >= chars.len() { return Err("unterminated string".into()); }
            pos += 1;
            tokens.push(Token::StringLiteral(s));
            continue;
        }

        // Numbers
        if ch.is_ascii_digit() {
            let start = pos;
            while pos < chars.len() && chars[pos].is_ascii_digit() { pos += 1; }
            if pos < chars.len() && chars[pos] == '.' {
                pos += 1;
                while pos < chars.len() && chars[pos].is_ascii_digit() { pos += 1; }
                let s: String = chars[start..pos].iter().collect();
                tokens.push(Token::FloatLiteral(s.parse().unwrap()));
            } else {
                let s: String = chars[start..pos].iter().collect();
                tokens.push(Token::IntegerLiteral(s.parse().unwrap()));
            }
            continue;
        }

        // Identifiers / keywords
        if ch.is_alphabetic() || ch == '_' {
            let start = pos;
            while pos < chars.len() && (chars[pos].is_alphanumeric() || chars[pos] == '_') { pos += 1; }
            let word: String = chars[start..pos].iter().collect();
            let tok = match word.to_uppercase().as_str() {
                "SELECT" => Token::Select, "FROM" => Token::From,
                "WHERE" => Token::Where, "INSERT" => Token::Insert,
                "INTO" => Token::Into, "VALUES" => Token::Values,
                "UPDATE" => Token::Update, "SET" => Token::Set,
                "DELETE" => Token::Delete, "CREATE" => Token::Create,
                "DROP" => Token::Drop, "TABLE" => Token::Table,
                "INDEX" => Token::Index, "ON" => Token::On,
                "AND" => Token::And, "OR" => Token::Or,
                "NOT" => Token::Not, "NULL" => Token::Null,
                "IS" => Token::Is, "AS" => Token::As,
                "ORDER" => Token::Order, "BY" => Token::By,
                "ASC" => Token::Asc, "DESC" => Token::Desc,
                "LIMIT" => Token::Limit, "PRIMARY" => Token::Primary,
                "KEY" => Token::Key, "INT" => Token::Int,
                "INTEGER" => Token::Integer, "BIGINT" => Token::Bigint,
                "FLOAT" => Token::Float, "DOUBLE" => Token::Double,
                "VARCHAR" => Token::Varchar, "TEXT" => Token::Text,
                "BLOB" => Token::Blob, "BOOLEAN" | "BOOL" => Token::Boolean,
                "TRUE" => Token::True, "FALSE" => Token::False,
                "UNIQUE" => Token::Unique,
                _ => Token::Identifier(word),
            };
            tokens.push(tok);
            continue;
        }

        return Err(format!("unexpected char '{}' at {}", ch, pos));
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_select() {
        let t = tokenize("SELECT * FROM users WHERE id = 1").unwrap();
        assert_eq!(t[0], Token::Select);
        assert_eq!(t[1], Token::Star);
        assert_eq!(t[2], Token::From);
    }

    #[test]
    fn test_string_lit() {
        let t = tokenize("'hello world'").unwrap();
        assert_eq!(t[0], Token::StringLiteral("hello world".into()));
    }

    #[test]
    fn test_operators() {
        let t = tokenize("a >= 5 AND b <> 'x'").unwrap();
        assert!(t.contains(&Token::GtEq));
        assert!(t.contains(&Token::NotEq));
    }
}
