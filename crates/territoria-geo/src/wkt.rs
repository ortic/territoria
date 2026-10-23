//! A small parser for coordinate system WKT (WKT1, the ESRI flavour found in
//! `.prj` files, and WKT2).
//!
//! Only the tree is parsed here; [`crate::crs`] interprets it.

/// One `KEYWORD[arg, arg, …]` node.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub keyword: String,
    pub args: Vec<Arg>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Text(String),
    Number(f64),
    /// A bare word, e.g. `EAST` in `AXIS["x",EAST]`.
    Word(String),
    Node(Node),
}

impl Node {
    /// The first quoted argument, usually the name.
    pub fn name(&self) -> Option<&str> {
        self.args.iter().find_map(|a| match a {
            Arg::Text(t) => Some(t.as_str()),
            _ => None,
        })
    }

    /// Direct children with one of the keywords (case-insensitive).
    pub fn children<'a>(&'a self, keywords: &[&str]) -> impl Iterator<Item = &'a Node> + 'a {
        let keywords: Vec<String> = keywords.iter().map(|k| k.to_ascii_uppercase()).collect();
        self.args.iter().filter_map(move |a| match a {
            Arg::Node(n) if keywords.iter().any(|k| n.keyword.eq_ignore_ascii_case(k)) => Some(n),
            _ => None,
        })
    }

    pub fn child(&self, keywords: &[&str]) -> Option<&Node> {
        self.children(keywords).next()
    }

    /// Numeric arguments in order.
    pub fn numbers(&self) -> Vec<f64> {
        self.args
            .iter()
            .filter_map(|a| match a {
                Arg::Number(n) => Some(*n),
                _ => None,
            })
            .collect()
    }

    /// Text and number arguments as strings, e.g. `AUTHORITY["EPSG","2056"]`
    /// and `ID["EPSG",2056]` both give `["EPSG", "2056"]`.
    pub fn values(&self) -> Vec<String> {
        self.args
            .iter()
            .filter_map(|a| match a {
                Arg::Text(t) => Some(t.clone()),
                Arg::Number(n) => Some(format!("{n}")),
                Arg::Word(w) => Some(w.clone()),
                Arg::Node(_) => None,
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid WKT: {}", self.0)
    }
}

impl std::error::Error for ParseError {}

pub fn parse(text: &str) -> Result<Node, ParseError> {
    let mut p = Parser {
        s: text.trim().as_bytes(),
        pos: 0,
    };
    let node = p.node()?;
    // ESRI writes a vertical CRS after the horizontal one (`PROJCS[…],VERTCS[…]`);
    // only the first is used.
    while p.peek() == Some(b',') {
        p.pos += 1;
        p.node()?;
    }
    if p.peek().is_some() {
        return Err(ParseError(format!("unexpected text at {}", p.pos)));
    }
    Ok(node)
}

struct Parser<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn skip_ws(&mut self) {
        while self.pos < self.s.len() && self.s[self.pos].is_ascii_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.s.get(self.pos).copied()
    }

    fn word(&mut self) -> String {
        self.skip_ws();
        let start = self.pos;
        while self.pos < self.s.len()
            && (self.s[self.pos].is_ascii_alphanumeric() || self.s[self.pos] == b'_')
        {
            self.pos += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.pos]).into_owned()
    }

    fn node(&mut self) -> Result<Node, ParseError> {
        let keyword = self.word();
        if keyword.is_empty() {
            return Err(ParseError(format!("keyword expected at {}", self.pos)));
        }
        let close = match self.peek() {
            Some(b'[') => b']',
            Some(b'(') => b')',
            _ => {
                return Ok(Node {
                    keyword,
                    args: Vec::new(),
                });
            }
        };
        self.pos += 1;
        let mut args = Vec::new();
        loop {
            match self.peek() {
                None => return Err(ParseError(format!("`{keyword}` is not closed"))),
                Some(c) if c == close => {
                    self.pos += 1;
                    break;
                }
                Some(b',') => self.pos += 1,
                Some(b'"') => args.push(Arg::Text(self.quoted()?)),
                Some(c) if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => {
                    args.push(Arg::Number(self.number()?))
                }
                Some(_) => {
                    let save = self.pos;
                    let word = self.word();
                    if word.is_empty() {
                        return Err(ParseError(format!("unexpected character at {save}")));
                    }
                    if matches!(self.peek(), Some(b'[') | Some(b'(')) {
                        self.pos = save;
                        args.push(Arg::Node(self.node()?));
                    } else {
                        args.push(Arg::Word(word));
                    }
                }
            }
        }
        Ok(Node { keyword, args })
    }

    fn quoted(&mut self) -> Result<String, ParseError> {
        self.pos += 1;
        let mut out = Vec::new();
        loop {
            match self.s.get(self.pos) {
                None => return Err(ParseError("unterminated string".into())),
                // WKT escapes a quote by doubling it.
                Some(b'"') if self.s.get(self.pos + 1) == Some(&b'"') => {
                    out.push(b'"');
                    self.pos += 2;
                }
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(&c) => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Ok(String::from_utf8_lossy(&out).into_owned())
    }

    fn number(&mut self) -> Result<f64, ParseError> {
        let start = self.pos;
        while self.pos < self.s.len()
            && matches!(
                self.s[self.pos],
                b'0'..=b'9' | b'.' | b'-' | b'+' | b'e' | b'E'
            )
        {
            self.pos += 1;
        }
        let text = String::from_utf8_lossy(&self.s[start..self.pos]);
        text.parse()
            .map_err(|_| ParseError(format!("bad number `{text}`")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_esri_prj() {
        let n = parse(r#"PROJCS["CH1903+_LV95",GEOGCS["GCS_CH1903+",DATUM["D_CH1903+",SPHEROID["Bessel_1841",6377397.155,299.1528128]],PRIMEM["Greenwich",0.0],UNIT["Degree",0.0174532925199433]],PROJECTION["Hotine_Oblique_Mercator_Azimuth_Center"],PARAMETER["False_Easting",2600000.0],UNIT["Meter",1.0]]"#).unwrap();
        assert_eq!(n.keyword, "PROJCS");
        assert_eq!(n.name(), Some("CH1903+_LV95"));
        let geog = n.child(&["GEOGCS"]).unwrap();
        let sph = geog
            .child(&["DATUM"])
            .unwrap()
            .child(&["SPHEROID"])
            .unwrap();
        assert_eq!(sph.numbers(), vec![6377397.155, 299.1528128]);
        let p = n.child(&["PARAMETER"]).unwrap();
        assert_eq!(p.values(), vec!["False_Easting", "2600000"]);
    }

    #[test]
    fn parses_words_and_wkt2_ids() {
        let n = parse(r#"PROJCRS["x",CS[Cartesian,2],AXIS["(E)",east],ID["EPSG",2056]]"#).unwrap();
        assert_eq!(n.child(&["ID"]).unwrap().values(), vec!["EPSG", "2056"]);
        assert_eq!(
            n.child(&["CS"]).unwrap().args[0],
            Arg::Word("Cartesian".into())
        );
    }

    #[test]
    fn rejects_broken_text() {
        assert!(parse(r#"PROJCS["x""#).is_err());
        assert!(parse("PROJCS[\"x\"] trailing").is_err());
    }
}
