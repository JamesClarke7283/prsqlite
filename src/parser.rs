// Copyright 2023 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::record::Value;
use crate::token::get_token_no_space;
use crate::token::Token;
use crate::utils::CaseInsensitiveBytes;

pub type Error = &'static str;
pub type Result<T> = std::result::Result<T, Error>;

pub struct CreateTable<'a> {
    pub table_name: &'a [u8],
    pub columns: Vec<ColumnDef<'a>>,
}

/// Definition of a column in a table.
#[derive(Debug, PartialEq, Eq)]
pub struct ColumnDef<'a> {
    pub name: &'a [u8],
    pub data_type: Option<DataType>,
    pub primary_key: bool,
}

/// Data Type.
///
/// https://www.sqlite.org/datatype3.html
#[derive(Debug, PartialEq, Eq)]
pub enum DataType {
    Null,
    Integer,
    Real,
    Text,
    Blob,
}

/// Parse CREATE TABLE statement.
///
/// https://www.sqlite.org/lang_createtable.html
pub fn parse_create_table(input: &[u8]) -> Result<(usize, CreateTable)> {
    let mut input = input;
    let len_input = input.len();

    if let Some((n, Token::Create)) = get_token_no_space(input) {
        input = &input[n..];
    } else {
        return Err("no create");
    }
    if let Some((n, Token::Table)) = get_token_no_space(input) {
        input = &input[n..];
    } else {
        return Err("no table");
    }
    let table_name = if let Some((n, Token::Identifier(table_name))) = get_token_no_space(input) {
        input = &input[n..];
        table_name
    } else {
        return Err("no table_name");
    };
    if let Some((n, Token::LeftParen)) = get_token_no_space(input) {
        input = &input[n..];
    } else {
        return Err("no left paren");
    }
    let mut columns = Vec::new();
    loop {
        let name = if let Some((n, Token::Identifier(column_name))) = get_token_no_space(input) {
            input = &input[n..];
            column_name
        } else {
            return Err("no column name");
        };

        let (mut n, mut token) = get_token_no_space(input).ok_or("no right paren")?;
        input = &input[n..];
        let data_type = match token {
            Token::Null => {
                (n, token) = get_token_no_space(input).ok_or("no right paren")?;
                input = &input[n..];
                Some(DataType::Null)
            }
            Token::Identifier(data_type) => {
                (n, token) = get_token_no_space(input).ok_or("no right paren")?;
                input = &input[n..];

                // TODO: compare the performance of UpperToLowerBytes::equal_to_lower_bytes or match + [u8;7]
                let data_type = CaseInsensitiveBytes::from(data_type);
                let data_type = if data_type.equal_to_lower_bytes(b"integer") {
                    DataType::Integer
                } else if data_type.equal_to_lower_bytes(b"real") {
                    DataType::Real
                } else if data_type.equal_to_lower_bytes(b"text") {
                    DataType::Text
                } else if data_type.equal_to_lower_bytes(b"blob") {
                    DataType::Blob
                } else {
                    return Err("unknown data type");
                };
                Some(data_type)
            }
            _ => None,
        };

        let primary_key = if let Token::Primary = token {
            match get_token_no_space(input) {
                Some((n, Token::Key)) => {
                    input = &input[n..];
                }
                _ => return Err("no key"),
            }
            (n, token) = get_token_no_space(input).ok_or("no right paren")?;
            input = &input[n..];

            true
        } else {
            false
        };

        columns.push(ColumnDef {
            name,
            data_type,
            primary_key,
        });

        match token {
            Token::Comma => {
                input = &input[n..];
            }
            Token::RightParen => {
                break;
            }
            _ => return Err("no right paren"),
        }
    }

    Ok((len_input - input.len(), CreateTable { table_name, columns }))
}

pub struct Select<'a> {
    pub table_name: &'a [u8],
    pub columns: Vec<ResultColumn<'a>>,
    pub selection: Option<Expr<'a>>,
}

// Parse SELECT statement.
//
// https://www.sqlite.org/lang_select.html
pub fn parse_select(input: &[u8]) -> Result<(usize, Select)> {
    let mut input = input;
    let len_input = input.len();

    if let Some((n, Token::Select)) = get_token_no_space(input) {
        input = &input[n..];
    } else {
        return Err("no select");
    }
    let (n, result_column) = parse_result_column(input)?;
    input = &input[n..];
    let mut columns = vec![result_column];
    loop {
        match get_token_no_space(input) {
            Some((n, Token::Comma)) => {
                input = &input[n..];
                let (n, result_column) = parse_result_column(input)?;
                input = &input[n..];
                columns.push(result_column);
            }
            Some((n, Token::From)) => {
                input = &input[n..];
                break;
            }
            _ => return Err("no from"),
        }
    }
    let table_name = if let Some((n, Token::Identifier(table_name))) = get_token_no_space(input) {
        input = &input[n..];
        table_name
    } else {
        return Err("no table_name");
    };

    let selection = if let Some((n, Token::Where)) = get_token_no_space(input) {
        input = &input[n..];
        let (n, expr) = parse_expr(input)?;
        input = &input[n..];
        Some(expr)
    } else {
        None
    };

    Ok((
        len_input - input.len(),
        Select {
            table_name,
            columns,
            selection,
        },
    ))
}

#[derive(Debug, PartialEq, Eq)]
pub enum ResultColumn<'a> {
    All,
    ColumnName(&'a [u8]),
}

fn parse_result_column(input: &[u8]) -> Result<(usize, ResultColumn)> {
    match get_token_no_space(input) {
        Some((n, Token::Identifier(id))) => Ok((n, ResultColumn::ColumnName(id))),
        Some((n, Token::Asterisk)) => Ok((n, ResultColumn::All)),
        _ => Err("no result column name"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum BinaryOperator {
    /// Equal to
    Eq,
    /// Not equal to
    Ne,
    /// Greater than
    Gt,
    /// Greater than or equal to
    Ge,
    /// Less than
    Lt,
    /// Less than or equal to
    Le,
}

#[derive(Debug, PartialEq)]
pub enum Expr<'a> {
    Column(&'a [u8]),
    BinaryOperator {
        operator: BinaryOperator,
        left: Box<Expr<'a>>,
        right: Box<Expr<'a>>,
    },
    LiteralValue(Value<'a>),
}

fn parse_expr(input: &[u8]) -> Result<(usize, Expr)> {
    let input_len = input.len();
    let (n, left) = match get_token_no_space(input) {
        Some((n, Token::Identifier(id))) => (n, Expr::Column(id)),
        Some((n, Token::Integer(i))) => (n, Expr::LiteralValue(Value::Integer(i))),
        _ => return Err("no expr"),
    };
    let input = &input[n..];
    let (n, operator) = match get_token_no_space(input) {
        Some((n, Token::Eq)) => (n, BinaryOperator::Eq),
        Some((n, Token::Ne)) => (n, BinaryOperator::Ne),
        Some((n, Token::Gt)) => (n, BinaryOperator::Gt),
        Some((n, Token::Ge)) => (n, BinaryOperator::Ge),
        Some((n, Token::Lt)) => (n, BinaryOperator::Lt),
        Some((n, Token::Le)) => (n, BinaryOperator::Le),
        _ => return Ok((n, left)),
    };
    let input = &input[n..];

    let (n, right) = parse_expr(input)?;

    Ok((
        input_len - input.len() + n,
        Expr::BinaryOperator {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_create_table() {
        let input = b"create table foo (id integer primary key, name text, real real, blob blob, empty null, no_type)";
        let (n, create_table) = parse_create_table(input).unwrap();
        assert_eq!(n, input.len());
        assert_eq!(create_table.table_name, b"foo");
        assert_eq!(
            create_table.columns,
            vec![
                ColumnDef {
                    name: b"id",
                    data_type: Some(DataType::Integer),
                    primary_key: true,
                },
                ColumnDef {
                    name: b"name",
                    data_type: Some(DataType::Text),
                    primary_key: false,
                },
                ColumnDef {
                    name: b"real",
                    data_type: Some(DataType::Real),
                    primary_key: false,
                },
                ColumnDef {
                    name: b"blob",
                    data_type: Some(DataType::Blob),
                    primary_key: false,
                },
                ColumnDef {
                    name: b"empty",
                    data_type: Some(DataType::Null),
                    primary_key: false,
                },
                ColumnDef {
                    name: b"no_type",
                    data_type: None,
                    primary_key: false,
                },
            ]
        );
    }

    #[test]
    fn test_parse_create_table_with_extra() {
        let input = b"create table Foo (Id, Name)abc ";
        let (n, create_table) = parse_create_table(input).unwrap();
        assert_eq!(n, input.len() - 4);
        assert_eq!(create_table.table_name, b"Foo");
        assert_eq!(
            create_table.columns,
            vec![
                ColumnDef {
                    name: b"Id",
                    data_type: None,
                    primary_key: false,
                },
                ColumnDef {
                    name: b"Name",
                    data_type: None,
                    primary_key: false,
                }
            ]
        );
    }

    #[test]
    fn test_parse_create_table_fail() {
        // no right paren.
        assert!(parse_create_table(b"create table foo (id, name ").is_err());
        // invalid data_type.
        assert!(parse_create_table(b"create table foo (id, name invalid)").is_err());
        // primary without key.
        assert!(parse_create_table(b"create table foo (id primary, name)").is_err());
        // key without primary.
        assert!(parse_create_table(b"create table foo (id key, name)").is_err());
    }

    #[test]
    fn test_parse_select_all() {
        let input = b"select * from foo";
        let (n, select) = parse_select(input).unwrap();
        assert_eq!(n, input.len());
        assert_eq!(select.table_name, b"foo");
        assert_eq!(select.columns, vec![ResultColumn::All]);
    }

    #[test]
    fn test_parse_select_columns() {
        let input = b"select id,name,*,col from foo";
        let (n, select) = parse_select(input).unwrap();
        assert_eq!(n, input.len());
        assert_eq!(select.table_name, b"foo");
        assert_eq!(
            select.columns,
            vec![
                ResultColumn::ColumnName(b"id"),
                ResultColumn::ColumnName(b"name"),
                ResultColumn::All,
                ResultColumn::ColumnName(b"col")
            ]
        );
    }

    #[test]
    fn test_parse_select_where() {
        let input = b"select * from foo where id = 5";
        let (n, select) = parse_select(input).unwrap();
        assert_eq!(n, input.len());
        assert_eq!(select.table_name, b"foo");
        assert_eq!(select.columns, vec![ResultColumn::All,]);
        assert!(select.selection.is_some());
        assert_eq!(
            select.selection.unwrap(),
            Expr::BinaryOperator {
                operator: BinaryOperator::Eq,
                left: Box::new(Expr::Column(b"id")),
                right: Box::new(Expr::LiteralValue(Value::Integer(5))),
            }
        );
    }

    #[test]
    fn test_parse_select_fail() {
        // no table name.
        let input = b"select col from ";
        assert!(parse_create_table(input).is_err());
    }
}