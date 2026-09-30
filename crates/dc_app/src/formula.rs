use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Num(f64),
    Var(String),
    Op(char),
    LParen,
    RParen,
}

pub fn tokenize(expr: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();

    while let Some(&c) = chars.peek() {
        if c.is_whitespace() || c == '$' {
            chars.next();
            continue;
        }

        match c {
            '+' | '-' | '*' | '/' | '^' => {
                tokens.push(Token::Op(c));
                chars.next();
            }
            '(' => {
                tokens.push(Token::LParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::RParen);
                chars.next();
            }
            '[' => {
                chars.next(); // Consume '['
                let mut var_name = String::new();
                while let Some(&vc) = chars.peek() {
                    if vc == ']' {
                        chars.next();
                        break;
                    }
                    var_name.push(vc);
                    chars.next();
                }
                tokens.push(Token::Var(var_name));
            }
            _ if c.is_digit(10) || c == '.' => {
                let mut num_str = String::new();
                while let Some(&nc) = chars.peek() {
                    if nc.is_digit(10) || nc == '.' {
                        num_str.push(nc);
                        chars.next();
                    } else {
                        break;
                    }
                }
                let val = num_str
                    .parse::<f64>()
                    .map_err(|_| format!("Invalid number: {}", num_str))?;
                tokens.push(Token::Num(val));
            }
            _ => return Err(format!("Unexpected character: {}", c)),
        }
    }
    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    vars: &'a HashMap<String, f64>,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token], vars: &'a HashMap<String, f64>) -> Self {
        Self {
            tokens,
            pos: 0,
            vars,
        }
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<&'a Token> {
        let tok = self.tokens.get(self.pos);
        self.pos += 1;
        tok
    }

    fn parse_expr(&mut self) -> Result<f64, String> {
        self.parse_add_sub()
    }

    fn parse_add_sub(&mut self) -> Result<f64, String> {
        let mut left = self.parse_mul_div()?;
        while let Some(tok) = self.peek() {
            if let Token::Op(op @ '+') | Token::Op(op @ '-') = tok {
                let op = *op;
                self.next();
                let right = self.parse_mul_div()?;
                left = if op == '+' {
                    left + right
                } else {
                    left - right
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_mul_div(&mut self) -> Result<f64, String> {
        let mut left = self.parse_pow()?;
        while let Some(tok) = self.peek() {
            if let Token::Op(op @ '*') | Token::Op(op @ '/') = tok {
                let op = *op;
                self.next();
                let right = self.parse_pow()?;
                left = if op == '*' {
                    left * right
                } else {
                    left / right
                };
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_pow(&mut self) -> Result<f64, String> {
        let mut left = self.parse_primary()?;
        while let Some(tok) = self.peek() {
            if let Token::Op('^') = tok {
                self.next();
                let right = self.parse_primary()?;
                left = left.powf(right);
            } else {
                break;
            }
        }
        Ok(left)
    }

    fn parse_primary(&mut self) -> Result<f64, String> {
        let tok = self.next().ok_or("Unexpected end of expression")?;
        match tok {
            Token::Num(n) => Ok(*n),
            Token::Var(name) => {
                let val = self.vars.get(name.trim()).copied().unwrap_or(0.0);
                Ok(val)
            }
            Token::LParen => {
                let val = self.parse_expr()?;
                match self.next() {
                    Some(Token::RParen) => Ok(val),
                    _ => return Err("Expected ')'".to_string()),
                }
            }
            Token::Op('-') => {
                let val = self.parse_primary()?;
                Ok(-val)
            }
            _ => Err(format!("Unexpected token: {:?}", tok)),
        }
    }
}

/// Evaluates a formula and returns the result. Variables must be enclosed in square brackets `[var]`.
pub fn evaluate_formula(expr: &str, vars: &HashMap<String, f64>) -> Result<f64, String> {
    if expr.trim().is_empty() {
        return Ok(0.0);
    }
    let tokens = tokenize(expr)?;
    if tokens.is_empty() {
        return Ok(0.0);
    }
    let mut parser = Parser::new(&tokens, vars);
    let val = parser.parse_expr()?;
    if parser.pos < tokens.len() {
        return Err("Unexpected trailing tokens".to_string());
    }
    Ok(val)
}
