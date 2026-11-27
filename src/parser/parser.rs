//! # SQL Parser
//!
//! Recursive-descent parser that converts tokens into AST nodes.

use super::ast::*;
use super::lexer::Token;
use crate::catalog::schema::DataType;

/// Parser state.
pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn advance(&mut self) -> Token {
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), String> {
        let tok = self.advance();
        if std::mem::discriminant(&tok) == std::mem::discriminant(expected) {
            Ok(())
        } else {
            Err(format!("expected {:?}, got {:?}", expected, tok))
        }
    }

    fn match_token(&mut self, expected: &Token) -> bool {
        if std::mem::discriminant(self.peek()) == std::mem::discriminant(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    /// Parse a single SQL statement.
    pub fn parse(&mut self) -> Result<Statement, String> {
        let stmt = match self.peek() {
            Token::Select => self.parse_select(),
            Token::Insert => self.parse_insert(),
            Token::Update => self.parse_update(),
            Token::Delete => self.parse_delete(),
            Token::Create => {
                self.advance();
                match self.peek() {
                    Token::Table => self.parse_create_table(),
                    Token::Index | Token::Unique => self.parse_create_index(),
                    _ => Err("expected TABLE or INDEX after CREATE".into()),
                }
            }
            Token::Drop => self.parse_drop_table(),
            Token::Begin => {
                self.advance();
                if *self.peek() == Token::Transaction {
                    self.advance();
                }
                Ok(Statement::BeginTransaction)
            }
            Token::Commit => {
                self.advance();
                Ok(Statement::Commit)
            }
            Token::Rollback => {
                self.advance();
                Ok(Statement::Rollback)
            }
            _ => Err(format!("unexpected token: {:?}", self.peek())),
        };
        // Optional semicolon
        if self.pos < self.tokens.len() && *self.peek() == Token::Semicolon {
            self.advance();
        }
        stmt
    }

    fn parse_select(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Select)?;

        // Columns
        let mut columns = Vec::new();
        if *self.peek() == Token::Star {
            self.advance();
            columns.push(SelectColumn::Wildcard);
        } else {
            loop {
                let expr = self.parse_expr()?;
                let alias = if self.match_token(&Token::As) {
                    if let Token::Identifier(name) = self.advance() {
                        Some(name)
                    } else {
                        return Err("expected alias name".into());
                    }
                } else {
                    None
                };
                columns.push(SelectColumn::Expr { expr, alias });
                if !self.match_token(&Token::Comma) { break; }
            }
        }

        self.expect(&Token::From)?;
        let from = if let Token::Identifier(name) = self.advance() {
            name
        } else {
            return Err("expected table name".into());
        };

        // JOIN clauses: [INNER | LEFT | RIGHT] JOIN table ON expr
        let mut joins = Vec::new();
        while *self.peek() == Token::Join || *self.peek() == Token::Inner || *self.peek() == Token::Left || *self.peek() == Token::Right {
            if *self.peek() == Token::Inner || *self.peek() == Token::Left || *self.peek() == Token::Right {
                self.advance();
            }
            self.expect(&Token::Join)?;
            let join_table = if let Token::Identifier(name) = self.advance() {
                name
            } else {
                return Err("expected table name after JOIN".into());
            };
            self.expect(&Token::On)?;
            let on_expr = self.parse_expr()?;
            joins.push(JoinClause { table: join_table, on: on_expr });
        }

        let where_clause = if *self.peek() == Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        let order_by = if *self.peek() == Token::Order {
            self.advance();
            self.expect(&Token::By)?;
            let mut items = Vec::new();
            loop {
                let col = if let Token::Identifier(n) = self.advance() {
                    n
                } else {
                    return Err("expected column in ORDER BY".into());
                };
                let asc = if *self.peek() == Token::Desc {
                    self.advance(); false
                } else {
                    if *self.peek() == Token::Asc { self.advance(); }
                    true
                };
                items.push(OrderByItem { column: col, ascending: asc });
                if !self.match_token(&Token::Comma) { break; }
            }
            Some(items)
        } else {
            None
        };

        let limit = if *self.peek() == Token::Limit {
            self.advance();
            if let Token::IntegerLiteral(n) = self.advance() {
                Some(n as usize)
            } else {
                return Err("expected integer after LIMIT".into());
            }
        } else {
            None
        };

        Ok(Statement::Select { columns, from, joins, where_clause, order_by, limit })
    }

    fn parse_insert(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Insert)?;
        self.expect(&Token::Into)?;

        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };

        // Optional column list
        let columns = if *self.peek() == Token::LeftParen {
            self.advance();
            let mut cols = Vec::new();
            loop {
                if let Token::Identifier(n) = self.advance() {
                    cols.push(n);
                } else {
                    return Err("expected column name".into());
                }
                if !self.match_token(&Token::Comma) { break; }
            }
            self.expect(&Token::RightParen)?;
            Some(cols)
        } else {
            None
        };

        self.expect(&Token::Values)?;

        let mut values = Vec::new();
        loop {
            self.expect(&Token::LeftParen)?;
            let mut row = Vec::new();
            loop {
                row.push(self.parse_expr()?);
                if !self.match_token(&Token::Comma) { break; }
            }
            self.expect(&Token::RightParen)?;
            values.push(row);
            if !self.match_token(&Token::Comma) { break; }
        }

        Ok(Statement::Insert { table_name, columns, values })
    }

    fn parse_update(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Update)?;
        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };
        self.expect(&Token::Set)?;

        let mut assignments = Vec::new();
        loop {
            let col = if let Token::Identifier(n) = self.advance() {
                n
            } else {
                return Err("expected column name".into());
            };
            self.expect(&Token::Eq)?;
            let val = self.parse_expr()?;
            assignments.push((col, val));
            if !self.match_token(&Token::Comma) { break; }
        }

        let where_clause = if *self.peek() == Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Update { table_name, assignments, where_clause })
    }

    fn parse_delete(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Delete)?;
        self.expect(&Token::From)?;
        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };

        let where_clause = if *self.peek() == Token::Where {
            self.advance();
            Some(self.parse_expr()?)
        } else {
            None
        };

        Ok(Statement::Delete { table_name, where_clause })
    }

    fn parse_create_table(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Table)?;
        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };

        self.expect(&Token::LeftParen)?;
        let mut columns = Vec::new();
        loop {
            let name = if let Token::Identifier(n) = self.advance() {
                n
            } else {
                return Err("expected column name".into());
            };

            let data_type = self.parse_data_type()?;
            let mut nullable = true;
            let mut primary_key = false;

            loop {
                match self.peek() {
                    Token::Not => {
                        self.advance();
                        self.expect(&Token::Null)?;
                        nullable = false;
                    }
                    Token::Primary => {
                        self.advance();
                        self.expect(&Token::Key)?;
                        primary_key = true;
                        nullable = false;
                    }
                    _ => break,
                }
            }

            columns.push(ColumnSpec { name, data_type, nullable, primary_key });
            if !self.match_token(&Token::Comma) { break; }
        }
        self.expect(&Token::RightParen)?;

        Ok(Statement::CreateTable { table_name, columns })
    }

    fn parse_create_index(&mut self) -> Result<Statement, String> {
        let unique = if *self.peek() == Token::Unique {
            self.advance(); true
        } else {
            false
        };
        self.expect(&Token::Index)?;

        let index_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected index name".into());
        };
        self.expect(&Token::On)?;
        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };
        self.expect(&Token::LeftParen)?;
        let column_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected column name".into());
        };
        self.expect(&Token::RightParen)?;

        Ok(Statement::CreateIndex { index_name, table_name, column_name, unique })
    }

    fn parse_drop_table(&mut self) -> Result<Statement, String> {
        self.expect(&Token::Drop)?;
        self.expect(&Token::Table)?;
        let table_name = if let Token::Identifier(n) = self.advance() {
            n
        } else {
            return Err("expected table name".into());
        };
        Ok(Statement::DropTable { table_name })
    }

    fn parse_data_type(&mut self) -> Result<DataType, String> {
        let tok = self.advance();
        match tok {
            Token::Int | Token::Integer => Ok(DataType::Integer),
            Token::Bigint => Ok(DataType::BigInt),
            Token::Float => Ok(DataType::Float),
            Token::Double => Ok(DataType::Double),
            Token::Boolean => Ok(DataType::Boolean),
            Token::Text => Ok(DataType::Text),
            Token::Blob => Ok(DataType::Blob),
            Token::Varchar => {
                self.expect(&Token::LeftParen)?;
                if let Token::IntegerLiteral(n) = self.advance() {
                    self.expect(&Token::RightParen)?;
                    Ok(DataType::Varchar(n as usize))
                } else {
                    Err("expected size for VARCHAR".into())
                }
            }
            _ => Err(format!("expected data type, got {:?}", tok)),
        }
    }

    /// Parse an expression (handles AND/OR).
    fn parse_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;

        loop {
            match self.peek() {
                Token::And => {
                    self.advance();
                    let right = self.parse_comparison()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::And,
                        right: Box::new(right),
                    };
                }
                Token::Or => {
                    self.advance();
                    let right = self.parse_comparison()?;
                    left = Expr::BinaryOp {
                        left: Box::new(left),
                        op: BinaryOperator::Or,
                        right: Box::new(right),
                    };
                }
                _ => break,
            }
        }

        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let left = self.parse_addition()?;

        // IS NULL / IS NOT NULL
        if *self.peek() == Token::Is {
            self.advance();
            let negated = self.match_token(&Token::Not);
            self.expect(&Token::Null)?;
            return Ok(Expr::IsNull { expr: Box::new(left), negated });
        }

        let op = match self.peek() {
            Token::Eq => BinaryOperator::Eq,
            Token::NotEq => BinaryOperator::NotEq,
            Token::Lt => BinaryOperator::Lt,
            Token::LtEq => BinaryOperator::LtEq,
            Token::Gt => BinaryOperator::Gt,
            Token::GtEq => BinaryOperator::GtEq,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_addition()?;
        Ok(Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) })
    }

    fn parse_addition(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_primary()?;
        loop {
            let op = match self.peek() {
                Token::Plus => BinaryOperator::Plus,
                Token::Minus => BinaryOperator::Minus,
                Token::Star => BinaryOperator::Multiply,
                Token::Slash => BinaryOperator::Divide,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary()?;
            left = Expr::BinaryOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<Expr, String> {
        match self.peek().clone() {
            Token::IntegerLiteral(n) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Integer(n)))
            }
            Token::FloatLiteral(f) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::Float(f)))
            }
            Token::StringLiteral(s) => {
                self.advance();
                Ok(Expr::Literal(LiteralValue::String(s)))
            }
            Token::True => { self.advance(); Ok(Expr::Literal(LiteralValue::Boolean(true))) }
            Token::False => { self.advance(); Ok(Expr::Literal(LiteralValue::Boolean(false))) }
            Token::Null => { self.advance(); Ok(Expr::Literal(LiteralValue::Null)) }
            Token::Not => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::Not(Box::new(expr)))
            }
            Token::Identifier(name) => {
                self.advance();
                // Check for table.column dot notation
                let full_name = if *self.peek() == Token::Dot {
                    self.advance();
                    if let Token::Identifier(col) = self.advance() {
                        format!("{}.{}", name, col)
                    } else if *self.peek() == Token::Star {
                        self.advance();
                        format!("{}.*", name)
                    } else {
                        return Err("expected column name after '.'".into());
                    }
                } else {
                    name
                };

                // Check for function call
                if *self.peek() == Token::LeftParen {
                    self.advance();
                    let mut args = Vec::new();
                    if *self.peek() != Token::RightParen {
                        loop {
                            if *self.peek() == Token::Star {
                                self.advance();
                                args.push(Expr::ColumnRef("*".into()));
                            } else {
                                args.push(self.parse_expr()?);
                            }
                            if !self.match_token(&Token::Comma) { break; }
                        }
                    }
                    self.expect(&Token::RightParen)?;
                    Ok(Expr::Function { name: full_name, args })
                } else {
                    Ok(Expr::ColumnRef(full_name))
                }
            }
            Token::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&Token::RightParen)?;
                Ok(expr)
            }
            Token::Minus => {
                self.advance();
                if let Token::IntegerLiteral(n) = self.advance() {
                    Ok(Expr::Literal(LiteralValue::Integer(-n)))
                } else {
                    Err("expected number after -".into())
                }
            }
            _ => Err(format!("unexpected token in expr: {:?}", self.peek())),
        }
    }
}

/// Convenience: parse a SQL string into a Statement.
pub fn parse_sql(sql: &str) -> Result<Statement, String> {
    let tokens = super::lexer::tokenize(sql)?;
    let mut parser = Parser::new(tokens);
    parser.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_select() {
        let stmt = parse_sql("SELECT * FROM users WHERE id = 1").unwrap();
        match stmt {
            Statement::Select { from, .. } => assert_eq!(from, "users"),
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_insert() {
        let stmt = parse_sql("INSERT INTO users (id, name) VALUES (1, 'Alice')").unwrap();
        match stmt {
            Statement::Insert { table_name, values, .. } => {
                assert_eq!(table_name, "users");
                assert_eq!(values.len(), 1);
                assert_eq!(values[0].len(), 2);
            }
            _ => panic!("expected INSERT"),
        }
    }

    #[test]
    fn test_parse_create_table() {
        let stmt = parse_sql(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name VARCHAR(100) NOT NULL, email TEXT)"
        ).unwrap();
        match stmt {
            Statement::CreateTable { table_name, columns } => {
                assert_eq!(table_name, "users");
                assert_eq!(columns.len(), 3);
                assert!(columns[0].primary_key);
                assert!(!columns[1].nullable);
            }
            _ => panic!("expected CREATE TABLE"),
        }
    }

    #[test]
    fn test_parse_update() {
        let stmt = parse_sql("UPDATE users SET name = 'Bob' WHERE id = 1").unwrap();
        match stmt {
            Statement::Update { table_name, assignments, .. } => {
                assert_eq!(table_name, "users");
                assert_eq!(assignments.len(), 1);
            }
            _ => panic!("expected UPDATE"),
        }
    }

    #[test]
    fn test_parse_delete() {
        let stmt = parse_sql("DELETE FROM users WHERE id = 1").unwrap();
        match stmt {
            Statement::Delete { table_name, .. } => assert_eq!(table_name, "users"),
            _ => panic!("expected DELETE"),
        }
    }

    #[test]
    fn test_parse_order_by_limit() {
        let stmt = parse_sql("SELECT * FROM t ORDER BY name DESC LIMIT 10").unwrap();
        match stmt {
            Statement::Select { order_by, limit, .. } => {
                let ob = order_by.unwrap();
                assert_eq!(ob[0].column, "name");
                assert!(!ob[0].ascending);
                assert_eq!(limit, Some(10));
            }
            _ => panic!("expected SELECT"),
        }
    }

    #[test]
    fn test_parse_join() {
        let stmt = parse_sql("SELECT users.name, orders.amount FROM users INNER JOIN orders ON users.id = orders.user_id").unwrap();
        match stmt {
            Statement::Select { from, joins, .. } => {
                assert_eq!(from, "users");
                assert_eq!(joins.len(), 1);
                assert_eq!(joins[0].table, "orders");
            }
            _ => panic!("expected SELECT with JOIN"),
        }
    }

    #[test]
    fn test_parse_transaction() {
        assert_eq!(parse_sql("BEGIN TRANSACTION").unwrap(), Statement::BeginTransaction);
        assert_eq!(parse_sql("COMMIT").unwrap(), Statement::Commit);
        assert_eq!(parse_sql("ROLLBACK").unwrap(), Statement::Rollback);
    }
}
