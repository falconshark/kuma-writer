//! Word（.docx）文件的讀寫。
//!
//! 編輯區只有純文字，所以讀的時候把每個段落變成一行。存檔時不重新產生整份文件，
//! 而是拿新的文字和檔案裡現有的段落逐行比對：
//! - 沒改到的段落、表格、圖片、分節設定原封不動保留（連粗體、斜體等字元格式都在）
//! - 改過的段落沿用原段落的段落格式（標題、縮排、對齊）和最後一段文字的字型，但段內的混合格式會變成一致
//! - 新加的段落沿用前面最近一個內文段落的格式（不會跟著變成標題），就像在 Word 裡按 Enter
//!
//! 頁首頁尾、註腳、樣式、圖片檔等其他部分整包照抄。

use std::collections::HashSet;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use std::ops::Range;
use std::path::Path;
use std::time::{Duration, Instant};

use similar::{capture_diff_slices_deadline, Algorithm, DiffTag};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::config;

fn bad(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

fn zip_err(e: zip::result::ZipError) -> io::Error {
    match e {
        zip::result::ZipError::Io(e) => e,
        e => bad(format!("不是有效的 Word 文件（{e}）")),
    }
}

/// 讀出文件的文字，一個段落一行
pub fn read(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    let pkg = Package::open(&bytes)?;
    Ok(Parsed::new(&pkg.document)?.lines.join("\n"))
}

/// 存檔。檔案已經存在就保留原本的格式，只換掉改過的段落；不存在就建一份新的
pub fn write(path: &Path, text: &str) -> io::Result<()> {
    let bytes = match fs::read(path) {
        Ok(old) => Package::open(&old)?.rewrite(text)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => new_document(text)?,
        Err(e) => return Err(e),
    };
    config::write_atomic(path, bytes)
}

// ── 封裝（zip）──────────────────────────────────

struct Package<'a> {
    bytes: &'a [u8],
    /// 主文件在 zip 裡的路徑，通常是 word/document.xml
    document_name: String,
    document: String,
    /// 被當成標題的樣式編號
    headings: HashSet<String>,
}

impl<'a> Package<'a> {
    fn open(bytes: &'a [u8]) -> io::Result<Self> {
        let mut zip = ZipArchive::new(Cursor::new(bytes)).map_err(zip_err)?;
        let rels = read_entry(&mut zip, "_rels/.rels")?.ok_or_else(|| bad("不是有效的 Word 文件"))?;
        let document_name = main_document(&rels).unwrap_or_else(|| "word/document.xml".to_owned());
        let document = read_entry(&mut zip, &document_name)?.ok_or_else(|| bad("不是有效的 Word 文件"))?;
        // 樣式表照慣例和主文件放在同一個資料夾
        let styles_name = match document_name.rsplit_once('/') {
            Some((dir, _)) => format!("{dir}/styles.xml"),
            None => "styles.xml".to_owned(),
        };
        let headings = match read_entry(&mut zip, &styles_name)? {
            Some(styles) => heading_styles(&styles),
            None => HashSet::new(),
        };
        Ok(Package { bytes, document_name, document, headings })
    }

    /// 換掉主文件，其他部分照原樣複製（不重新壓縮）
    fn rewrite(&self, text: &str) -> io::Result<Vec<u8>> {
        let document = Parsed::new(&self.document)?.rebuild(text, &self.headings);
        let mut zip = ZipArchive::new(Cursor::new(self.bytes)).map_err(zip_err)?;
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let entry = zip.by_index_raw(i).map_err(zip_err)?;
            if entry.name().map_err(zip_err)? == self.document_name {
                drop(entry);
                out.start_file(&self.document_name, deflated()).map_err(zip_err)?;
                out.write_all(document.as_bytes())?;
            } else {
                out.raw_copy_file(entry).map_err(zip_err)?;
            }
        }
        Ok(out.finish().map_err(zip_err)?.into_inner())
    }
}

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn read_entry<R: Read + io::Seek>(zip: &mut ZipArchive<R>, name: &str) -> io::Result<Option<String>> {
    let mut entry = match zip.by_name(name) {
        Ok(e) => e,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(zip_err(e)),
    };
    let mut s = String::new();
    entry.read_to_string(&mut s)?;
    // XML 檔開頭可能有 BOM
    Ok(Some(s.strip_prefix('\u{FEFF}').map(str::to_owned).unwrap_or(s)))
}

/// 從 _rels/.rels 找出主文件的位置
fn main_document(rels: &str) -> Option<String> {
    Lexer::new(rels).filter_map(Result::ok).find_map(|(_, tok)| match tok {
        Tok::Open { name: "Relationship", attrs, .. }
            if attr(attrs, "Type").is_some_and(|t| t.ends_with("/officeDocument")) =>
        {
            attr(attrs, "Target").map(|t| t.trim_start_matches('/').to_owned())
        }
        _ => None,
    })
}

/// styles.xml 裡名稱是 heading 1～9、Title、Subtitle 的樣式。
/// 內建樣式的名稱不論 Word 是什麼語言都存成英文，樣式編號則不一定（中文版常是「1」「2」）
fn heading_styles(styles: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut id = None;
    for (_, tok) in Lexer::new(styles).filter_map(Result::ok) {
        if let Tok::Open { name, attrs, .. } = tok {
            match name {
                "w:style" => id = attr(attrs, "w:styleId"),
                "w:name" => {
                    let n = attr(attrs, "w:val").unwrap_or_default().to_lowercase();
                    if n.starts_with("heading") || n == "title" || n == "subtitle" {
                        out.extend(id.take());
                    }
                }
                _ => {}
            }
        }
    }
    out
}

// ── 主文件的解析 ───────────────────────────────

struct Para {
    /// 段落格式 <w:pPr>
    ppr: Option<Range<usize>>,
    /// 最後一段有字的文字格式 <w:rPr>
    rpr: Option<Range<usize>>,
    style: Option<String>,
    /// 直接放在 <w:body> 下，不在表格之類的裡面
    top: bool,
}

struct Block {
    /// 整個元素在 XML 裡的位置
    range: Range<usize>,
    /// 這個元素裡的文字是第幾行到第幾行，沒有文字的（分節設定、書籤）是空的
    lines: Range<usize>,
}

struct Parsed<'a> {
    xml: &'a str,
    lines: Vec<String>,
    /// 每一行屬於哪個段落
    line_para: Vec<usize>,
    /// 每一行屬於 <w:body> 底下的哪個元素
    line_block: Vec<usize>,
    paras: Vec<Para>,
    blocks: Vec<Block>,
    /// <w:body> 的內容在 XML 裡的位置
    body: Range<usize>,
}

/// 解析時正在讀的段落
struct OpenPara {
    depth: usize,
    lines: Vec<String>,
    ppr_start: usize,
    ppr: Option<Range<usize>>,
    rpr: Option<Range<usize>>,
    style: Option<String>,
    top: bool,
}

impl<'a> Parsed<'a> {
    fn new(xml: &'a str) -> io::Result<Self> {
        let mut p = Parsed {
            xml,
            lines: vec![],
            line_para: vec![],
            line_block: vec![],
            paras: vec![],
            blocks: vec![],
            body: 0..0,
        };
        let mut stack: Vec<&str> = vec![];
        let mut body_level = None;
        // 目前這個區塊的開頭位置，和它的第一行是第幾行
        let mut block_start = (0, 0);
        // 這一層以下的內容不算文字：文字方塊（不在正文的段落裡）、相容用的備份內容（和正文重複）
        let mut skip: Option<usize> = None;
        let mut para: Option<OpenPara> = None;
        let mut run_rpr: Option<Range<usize>> = None;
        let mut rpr_start = 0;

        for tok in Lexer::new(xml) {
            let (r, tok) = tok?;
            match tok {
                Tok::Open { name, attrs, empty } => {
                    let depth = stack.len();
                    let parent = stack.last().copied().unwrap_or("");
                    if body_level == Some(depth) {
                        block_start = (r.start, p.lines.len());
                    }
                    if name == "w:body" && body_level.is_none() {
                        body_level = Some(depth + 1);
                        p.body = r.end..r.end;
                    }
                    if skip.is_none() {
                        match name {
                            "w:txbxContent" | "mc:Fallback" if !empty => skip = Some(depth),
                            "w:p" => {
                                para = Some(OpenPara {
                                    depth,
                                    lines: vec![String::new()],
                                    ppr_start: 0,
                                    ppr: None,
                                    rpr: None,
                                    style: None,
                                    top: body_level == Some(depth),
                                });
                            }
                            "w:pPr" if parent == "w:p" => {
                                if let Some(pa) = &mut para {
                                    pa.ppr_start = r.start;
                                    if empty {
                                        pa.ppr = Some(r.clone());
                                    }
                                }
                            }
                            "w:pStyle" if parent == "w:pPr" => {
                                if let Some(pa) = &mut para {
                                    pa.style = attr(attrs, "w:val");
                                }
                            }
                            "w:r" => run_rpr = None,
                            "w:rPr" if parent == "w:r" => {
                                rpr_start = r.start;
                                if empty {
                                    run_rpr = Some(r.clone());
                                }
                            }
                            _ if parent == "w:r" => {
                                if let Some(pa) = &mut para {
                                    let line = pa.lines.last_mut().unwrap();
                                    match name {
                                        "w:t" => pa.rpr = run_rpr.clone(),
                                        "w:tab" => line.push('\t'),
                                        "w:noBreakHyphen" => line.push('-'),
                                        "w:cr" => pa.lines.push(String::new()),
                                        // 分頁、分欄不是換行
                                        "w:br" if !matches!(attr(attrs, "w:type").as_deref(), Some("page" | "column")) => {
                                            pa.lines.push(String::new())
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                    if empty {
                        p.close(name, depth, r.clone(), &mut para, body_level, block_start);
                    } else {
                        stack.push(name);
                    }
                }
                Tok::Close { name } => {
                    if stack.pop() != Some(name) {
                        return Err(bad("Word 文件的內容有誤"));
                    }
                    let depth = stack.len();
                    if skip == Some(depth) {
                        skip = None;
                    } else if skip.is_none() {
                        match name {
                            "w:pPr" => {
                                if let Some(pa) = para.as_mut().filter(|pa| pa.depth + 1 == depth) {
                                    pa.ppr = Some(pa.ppr_start..r.end);
                                }
                            }
                            "w:rPr" if stack.last() == Some(&"w:r") => run_rpr = Some(rpr_start..r.end),
                            "w:body" => p.body.end = r.start,
                            _ => {}
                        }
                        p.close(name, depth, r.clone(), &mut para, body_level, block_start);
                    }
                }
                Tok::Text(s) => {
                    if skip.is_none() && stack.last() == Some(&"w:t") {
                        if let Some(pa) = &mut para {
                            pa.lines.last_mut().unwrap().push_str(&unescape(s));
                        }
                    }
                }
                Tok::Other => {}
            }
        }
        if body_level.is_none() || p.body.end < p.body.start {
            return Err(bad("Word 文件裡找不到內文"));
        }
        Ok(p)
    }

    /// 元素結束：段落收進清單，<w:body> 的直接子元素記成一個區塊
    fn close(
        &mut self,
        name: &str,
        depth: usize,
        r: Range<usize>,
        para: &mut Option<OpenPara>,
        body_level: Option<usize>,
        (block_start, first_line): (usize, usize),
    ) {
        if name == "w:p" && para.as_ref().is_some_and(|pa| pa.depth == depth) {
            let pa = para.take().unwrap();
            let i = self.paras.len();
            for line in pa.lines {
                self.lines.push(line);
                self.line_para.push(i);
                self.line_block.push(self.blocks.len());
            }
            self.paras.push(Para { ppr: pa.ppr, rpr: pa.rpr, style: pa.style, top: pa.top });
        }
        if body_level == Some(depth) {
            self.blocks.push(Block { range: block_start..r.end, lines: first_line..self.lines.len() });
        }
    }

    /// 依照新的文字重新組出主文件
    fn rebuild(&self, text: &str, headings: &HashSet<String>) -> String {
        let old: Vec<&str> = self.lines.iter().map(String::as_str).collect();
        let new: Vec<&str> = text.split('\n').collect();
        // 文件很長、改得很多時不追求最佳比對，避免存檔卡住
        let deadline = Instant::now() + Duration::from_millis(500);
        let ops = capture_diff_slices_deadline(Algorithm::Myers, &old, &new, Some(deadline));

        let mut b = Builder { doc: self, headings, out: String::new(), next_block: 0, body_para: None };
        for op in ops {
            let (tag, o, n) = op.as_tag_tuple();
            match tag {
                DiffTag::Equal => {
                    let mut i = o.start;
                    while i < o.end {
                        let block = self.line_block[i];
                        let lines = &self.blocks[block].lines;
                        b.reach(block);
                        if lines.start == i && lines.end <= o.end {
                            // 整個區塊都沒改：原封不動
                            b.out.push_str(&self.xml[self.blocks[block].range.clone()]);
                            b.next_block = block + 1;
                            for j in lines.clone() {
                                b.pass(j);
                            }
                            i = lines.end;
                        } else {
                            b.para(new[n.start + i - o.start], Some(i));
                            b.pass(i);
                            i += 1;
                        }
                    }
                }
                DiffTag::Delete | DiffTag::Insert | DiffTag::Replace => {
                    // 新舊一行對一行的當作改寫，沿用原段落的格式；多出來的新行當作新段落
                    for k in 0..n.len().max(o.len()) {
                        let i = o.start + k;
                        if i < o.end {
                            b.reach(self.line_block[i]);
                        }
                        if k < n.len() {
                            b.para(new[n.start + k], (i < o.end).then_some(i));
                        }
                        if i < o.end {
                            b.pass(i);
                        }
                    }
                }
            }
        }
        b.reach(self.blocks.len());
        let out = b.out;
        format!("{}{out}{}", &self.xml[..self.body.start], &self.xml[self.body.end..])
    }
}

struct Builder<'a> {
    doc: &'a Parsed<'a>,
    headings: &'a HashSet<String>,
    out: String,
    /// 下一個還沒處理到的區塊
    next_block: usize,
    /// 目前位置之前最近的內文段落，新段落沿用它的格式
    body_para: Option<usize>,
}

impl Builder<'_> {
    /// 走到某個區塊之前：中間沒有文字的區塊（分節設定、書籤等）照原位置保留
    fn reach(&mut self, block: usize) {
        while self.next_block < block {
            let b = &self.doc.blocks[self.next_block];
            if b.lines.is_empty() {
                self.out.push_str(&self.doc.xml[b.range.clone()]);
            }
            self.next_block += 1;
        }
    }

    /// 舊的第 i 行已經處理過
    fn pass(&mut self, i: usize) {
        let pi = self.doc.line_para[i];
        let pa = &self.doc.paras[pi];
        let heading = pa.style.as_ref().is_some_and(|s| self.headings.contains(s))
            || pa.ppr.as_ref().is_some_and(|r| self.doc.xml[r.clone()].contains("<w:outlineLvl"));
        if pa.top && !heading {
            self.body_para = Some(pi);
        }
    }

    /// 寫一個新段落。replaces 是它取代的舊行，格式照那一行的段落；
    /// 沒有的話是新加的段落，格式照前面最近的內文段落
    fn para(&mut self, text: &str, replaces: Option<usize>) {
        let doc = self.doc;
        let xml = doc.xml;
        let pi = replaces.map(|i| doc.line_para[i]).or(self.body_para);
        let tpl = pi.map(|i| &doc.paras[i]);
        let part = |r: Option<&Range<usize>>, strip: &[&str]| {
            r.map(|r| strip.iter().fold(xml[r.clone()].to_owned(), |s, name| strip_element(&s, name)))
                .unwrap_or_default()
        };
        // 段落裡的分節設定代表分節符號，只留給取代原段落最後一行的那個新段落，不然會多出分節；
        // 修訂記錄則一律不複製
        let ends_para = replaces.is_some_and(|i| doc.line_para.get(i + 1) != Some(&doc.line_para[i]));
        let ppr_strip: &[&str] = if ends_para { &["w:pPrChange"] } else { &["w:sectPr", "w:pPrChange"] };
        let ppr = part(tpl.and_then(|p| p.ppr.as_ref()), ppr_strip);
        let rpr = part(tpl.and_then(|p| p.rpr.as_ref()), &["w:rPrChange"]);
        self.out.push_str("<w:p>");
        self.out.push_str(&ppr);
        // XML 不允許大部分的控制字元，留著會讓 Word 打不開
        let text: String = text.chars().filter(|&c| c == '\t' || !c.is_control()).collect();
        if !text.is_empty() {
            self.out.push_str("<w:r>");
            self.out.push_str(&rpr);
            for (k, piece) in text.split('\t').enumerate() {
                if k > 0 {
                    self.out.push_str("<w:tab/>");
                }
                if !piece.is_empty() {
                    self.out.push_str("<w:t xml:space=\"preserve\">");
                    self.out.push_str(&escape(piece));
                    self.out.push_str("</w:t>");
                }
            }
            self.out.push_str("</w:r>");
        }
        self.out.push_str("</w:p>");
    }
}

/// 拿掉某個子元素（這些元素不會自己包自己）
fn strip_element(xml: &str, name: &str) -> String {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut s = xml.to_owned();
    let mut from = 0;
    while let Some(at) = s[from..].find(&open).map(|i| from + i) {
        let after = s[at + open.len()..].chars().next();
        if !matches!(after, Some(' ' | '>' | '/' | '\t' | '\r' | '\n')) {
            from = at + open.len();
            continue;
        }
        let Some(gt) = s[at..].find('>').map(|i| at + i) else { break };
        let end = if s[..gt].ends_with('/') {
            gt + 1
        } else {
            match s[gt..].find(&close) {
                Some(i) => gt + i + close.len(),
                None => break,
            }
        };
        s.replace_range(at..end, "");
        from = at;
    }
    s
}

// ── 新文件 ─────────────────────────────────────

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const DOCUMENT_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#;

/// 預設 12 點字、繁體中文
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:sz w:val="24"/><w:szCs w:val="24"/><w:lang w:val="en-US" w:eastAsia="zh-TW"/></w:rPr></w:rPrDefault><w:pPrDefault/></w:docDefaults><w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style></w:styles>"#;

/// A4 直式，邊界和 Word 的預設相同
const EMPTY_DOCUMENT: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1800" w:bottom="1440" w:left="1800" w:header="851" w:footer="992" w:gutter="0"/></w:sectPr></w:body></w:document>"#;

fn new_document(text: &str) -> io::Result<Vec<u8>> {
    let document = Parsed::new(EMPTY_DOCUMENT)?.rebuild(text, &HashSet::new());
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in [
        ("[Content_Types].xml", CONTENT_TYPES),
        ("_rels/.rels", ROOT_RELS),
        ("word/document.xml", document.as_str()),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS),
        ("word/styles.xml", STYLES),
    ] {
        out.start_file(name, deflated()).map_err(zip_err)?;
        out.write_all(data.as_bytes())?;
    }
    Ok(out.finish().map_err(zip_err)?.into_inner())
}

// ── 簡單的 XML 斷詞 ─────────────────────────────
// 存檔時要原封不動地保留沒改到的部分，所以需要每個標籤在原文裡的確切位置

enum Tok<'a> {
    Open { name: &'a str, attrs: &'a str, empty: bool },
    Close { name: &'a str },
    Text(&'a str),
    /// 宣告、註解、處理指令
    Other,
}

struct Lexer<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Lexer<'a> {
    fn new(s: &'a str) -> Self {
        Lexer { s, pos: 0 }
    }

    fn until(&self, from: usize, pat: &str) -> io::Result<usize> {
        self.s[from..].find(pat).map(|i| from + i + pat.len()).ok_or_else(|| bad("Word 文件的內容有誤"))
    }
}

impl<'a> Iterator for Lexer<'a> {
    type Item = io::Result<(Range<usize>, Tok<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.s;
        let start = self.pos;
        if start >= s.len() {
            return None;
        }
        let rest = &s[start..];
        if !rest.starts_with('<') {
            let end = rest.find('<').map_or(s.len(), |i| start + i);
            self.pos = end;
            return Some(Ok((start..end, Tok::Text(&s[start..end]))));
        }
        let result = (|| {
            if rest.starts_with("<![CDATA[") {
                let end = self.until(start, "]]>")?;
                return Ok((end, Tok::Text(&s[start + 9..end - 3])));
            }
            if rest.starts_with("<!--") {
                return Ok((self.until(start, "-->")?, Tok::Other));
            }
            if rest.starts_with("<?") {
                return Ok((self.until(start, "?>")?, Tok::Other));
            }
            if rest.starts_with("<!") {
                return Ok((self.until(start, ">")?, Tok::Other));
            }
            // 找標籤結尾，屬性值裡可能有 >
            let mut quote = None;
            let mut gt = None;
            for (i, c) in rest.char_indices().skip(1) {
                match (quote, c) {
                    (None, '"' | '\'') => quote = Some(c),
                    (Some(q), _) if c == q => quote = None,
                    (None, '>') => {
                        gt = Some(start + i);
                        break;
                    }
                    _ => {}
                }
            }
            let gt = gt.ok_or_else(|| bad("Word 文件的內容有誤"))?;
            let inner = &s[start + 1..gt];
            if let Some(name) = inner.strip_prefix('/') {
                return Ok((gt + 1, Tok::Close { name: name.trim() }));
            }
            let (inner, empty) = match inner.strip_suffix('/') {
                Some(i) => (i, true),
                None => (inner, false),
            };
            let split = inner.find(|c: char| c.is_whitespace()).unwrap_or(inner.len());
            Ok((gt + 1, Tok::Open { name: &inner[..split], attrs: &inner[split..], empty }))
        })();
        Some(result.map(|(end, tok)| {
            self.pos = end;
            (start..end, tok)
        }))
    }
}

/// 取出屬性值
fn attr(attrs: &str, key: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        rest = rest.trim_start();
        let eq = rest.find('=')?;
        let name = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let q = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let end = after[1..].find(q)? + 1;
        if name == key {
            return Some(unescape(&after[1..end]));
        }
        rest = &after[end + 1..];
    }
}

fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(semi) = rest.find(';') else { break };
        let name = &rest[1..semi];
        let c = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .map(|h| u32::from_str_radix(h, 16))
                .or_else(|| name.strip_prefix('#').map(str::parse))
                .and_then(Result::ok)
                .and_then(char::from_u32),
        };
        match c {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    const NS: &str = r#"xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main""#;

    fn doc(body: &str) -> String {
        format!(r#"<?xml version="1.0"?><w:document {NS}><w:body>{body}<w:sectPr><w:pgSz/></w:sectPr></w:body></w:document>"#)
    }

    fn text_of(xml: &str) -> String {
        Parsed::new(xml).unwrap().lines.join("\n")
    }

    fn headings() -> HashSet<String> {
        ["1".to_owned()].into()
    }

    const HEADING: &str = r#"<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t>第一章</w:t></w:r></w:p>"#;
    const BODY: &str = r#"<w:p><w:pPr><w:ind w:firstLineChars="200"/></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>粗</w:t></w:r><w:r><w:t xml:space="preserve">體 &amp; 正常</w:t></w:r></w:p>"#;

    #[test]
    fn reads_paragraphs_tabs_and_breaks() {
        let xml = doc(concat!(
            r#"<w:p><w:pPr><w:tabs><w:tab w:val="left" w:pos="720"/></w:tabs></w:pPr>"#,
            r#"<w:r><w:t>a</w:t><w:tab/><w:t>b</w:t><w:br/><w:t>c</w:t><w:br w:type="page"/></w:r></w:p>"#,
            r#"<w:p/>"#,
            r#"<w:p><w:r><w:t>&#x4E2D;&#25991;</w:t></w:r><w:r><w:delText>刪掉的</w:delText></w:r></w:p>"#,
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>格子</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#,
            r#"<w:p><w:r><mc:AlternateContent><mc:Choice><w:drawing><w:txbxContent><w:p><w:r><w:t>框</w:t></w:r></w:p></w:txbxContent></w:drawing></mc:Choice>"#,
            r#"<mc:Fallback><w:pict><w:p><w:r><w:t>框</w:t></w:r></w:p></w:pict></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t>圖後</w:t></w:r></w:p>"#,
        ));
        assert_eq!(text_of(&xml), "a\tb\nc\n\n中文\n格子\n圖後");
    }

    #[test]
    fn unchanged_text_keeps_the_file_identical() {
        let table = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>格子</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let xml = doc(&format!("{HEADING}{BODY}{table}"));
        let p = Parsed::new(&xml).unwrap();
        let text = p.lines.join("\n");
        assert_eq!(p.rebuild(&text, &headings()), xml);
    }

    #[test]
    fn edited_paragraph_keeps_its_paragraph_format() {
        let xml = doc(&format!("{HEADING}{BODY}"));
        let out = Parsed::new(&xml).unwrap().rebuild("第一章　雪\n粗體 & 正常", &headings());
        assert!(out.contains(r#"<w:p><w:pPr><w:pStyle w:val="1"/></w:pPr><w:r><w:t xml:space="preserve">第一章　雪</w:t></w:r></w:p>"#));
        assert!(out.contains(BODY), "{out}");
        assert!(out.ends_with("<w:sectPr><w:pgSz/></w:sectPr></w:body></w:document>"));
    }

    #[test]
    fn new_paragraphs_follow_body_text_not_headings() {
        let xml = doc(&format!("{BODY}{HEADING}"));
        let out = Parsed::new(&xml).unwrap().rebuild("粗體 & 正常\n第一章\n新的一段 <1>", &headings());
        assert!(out.contains(concat!(
            r#"</w:p><w:p><w:pPr><w:ind w:firstLineChars="200"/></w:pPr><w:r>"#,
            r#"<w:t xml:space="preserve">新的一段 &lt;1&gt;</w:t></w:r></w:p><w:sectPr>"#
        )), "{out}");
        assert_eq!(text_of(&out), "粗體 & 正常\n第一章\n新的一段 <1>");
    }

    #[test]
    fn editing_one_cell_turns_the_table_into_paragraphs() {
        let table = r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>甲</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>乙</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"#;
        let xml = doc(table);
        let out = Parsed::new(&xml).unwrap().rebuild("甲\n丙", &HashSet::new());
        assert!(!out.contains("<w:tbl>"));
        assert_eq!(text_of(&out), "甲\n丙");
    }

    #[test]
    fn section_breaks_stay_once_and_revisions_are_not_copied() {
        let p = r#"<w:p><w:pPr><w:jc w:val="center"/><w:sectPr><w:pgSz/></w:sectPr><w:pPrChange w:id="1"><w:pPr/></w:pPrChange></w:pPr><w:r><w:t>甲</w:t></w:r></w:p>"#;
        let out = Parsed::new(&doc(p)).unwrap().rebuild("乙
丙", &HashSet::new());
        assert!(out.contains(concat!(
            r#"<w:p><w:pPr><w:jc w:val="center"/><w:sectPr><w:pgSz/></w:sectPr></w:pPr><w:r><w:t xml:space="preserve">乙</w:t></w:r></w:p>"#,
            r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:t xml:space="preserve">丙</w:t></w:r></w:p>"#,
        )), "{out}");
    }

    #[test]
    fn soft_breaks_in_an_edited_paragraph_become_paragraphs() {
        let p = r#"<w:p><w:pPr><w:sectPr/></w:pPr><w:r><w:t>甲</w:t><w:br/><w:t>乙</w:t></w:r></w:p>"#;
        let out = Parsed::new(&doc(p)).unwrap().rebuild("甲
丁", &HashSet::new());
        assert_eq!(out.matches("<w:sectPr/>").count(), 1, "{out}");
        assert_eq!(text_of(&out), "甲
丁");
    }

    #[test]
    fn control_characters_are_dropped() {
        let out = Parsed::new(&doc("")).unwrap().rebuild("a\u{1}b\tc", &HashSet::new());
        assert!(out.contains("<w:t xml:space=\"preserve\">ab</w:t><w:tab/><w:t xml:space=\"preserve\">c</w:t>"));
    }

    #[test]
    fn finds_heading_styles_and_main_document() {
        let styles = r#"<w:styles><w:style w:styleId="1"><w:name w:val="heading 1"/></w:style><w:style w:styleId="a"><w:name w:val="Normal"/></w:style><w:style w:styleId="T"><w:name w:val="Title"/></w:style></w:styles>"#;
        let h = heading_styles(styles);
        assert!(h.contains("1") && h.contains("T") && !h.contains("a"));
        let rels = r#"<Relationships><Relationship Id="rId2" Type="x/core-properties" Target="docProps/core.xml"/><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="/word/document2.xml"/></Relationships>"#;
        assert_eq!(main_document(rels).as_deref(), Some("word/document2.xml"));
    }

    #[test]
    fn writes_new_files_and_edits_them_in_place() {
        let dir = std::env::temp_dir().join(format!("kuma-write-docx-{}", std::process::id()));
        let path = dir.join("稿子.docx");
        write(&path, "第一段\n\n第二段\t有 tab").unwrap();
        assert_eq!(read(&path).unwrap(), "第一段\n\n第二段\t有 tab");
        write(&path, "第一段\n\n第二段\t有 tab\n第三段").unwrap();
        assert_eq!(read(&path).unwrap(), "第一段\n\n第二段\t有 tab\n第三段");
        // 其他部分都還在
        let bytes = fs::read(&path).unwrap();
        let zip = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        assert_eq!(zip.len(), 5);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_to_overwrite_a_file_that_is_not_a_docx() {
        let dir = std::env::temp_dir().join(format!("kuma-write-notdocx-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.docx");
        fs::write(&path, "plain").unwrap();
        assert!(write(&path, "a").is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "plain");
        fs::remove_dir_all(&dir).unwrap();
    }
}
