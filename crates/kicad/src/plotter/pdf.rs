//! `PDF_PLOTTER` -- port of `common/plotters/PDF_plotter.cpp`: raw PDF
//! objects, an object-offset table and a classic `xref`/`trailer`, written
//! by hand exactly the way KiCad does (`allocPdfObject`, `startPdfObject`,
//! `startPdfStream`/`closePdfStream` with the deferred indirect `/Length`,
//! `StartPage`/`ClosePage`, `StartPlot`, `EndPlot`).
//!
//! Streams are written *uncompressed* -- the same path KiCad's own
//! `m_DebugPDFWriter` advanced option takes (`<< /Length N 0 R >>` with no
//! `/Filter`); this workspace has no zlib dependency.
//!
//! Not ported: `PDF_STROKE_FONT_MANAGER` / `PDF_OUTLINE_FONT_MANAGER` (text
//! is vector strokes, see this module's parent), hyperlink / property-popup
//! annotations, symbol bookmarks, embedded 3D, images, and the document
//! JavaScript (`/Names` points at an empty dictionary).

use super::*;
use std::io::Write as _;

/// `PDF_PLOTTER::OUTLINE_NODE`.
struct OutlineNode {
    title: String,
    entry_handle: i64,
    action_handle: i64,
    children: Vec<usize>,
}

pub struct PdfPlotter {
    base: PlotterBase,
    out: Vec<u8>,
    /// The page content stream being accumulated (`m_workFile`).
    work: Option<Vec<u8>>,
    xref: Vec<usize>,
    stream_length_handle: usize,
    page_stream_handle: i64,
    page_tree_handle: usize,
    font_res_dict_handle: usize,
    img_res_dict_handle: usize,
    js_names_handle: usize,
    page_handles: Vec<usize>,
    page_numbers: Vec<String>,
    page_name: String,
    parent_page_name: String,
    /// Arena of outline nodes; index 0 is the root.
    outline: Vec<OutlineNode>,
    total_outline_nodes: usize,
    plot_scale_adj: (f64, f64),
    /// Creation date as `D:YYYY:MM:DD:HH:MM:SS` (`/CreationDate`), passed in
    /// so the output is deterministic.
    creation_date: String,
}

/// `PDF_PLOTTER::encodeStringForPlotter` -- a PDF text string: a literal
/// `(...)` when it is plain ASCII, else a UTF-16BE hex string with BOM.
pub fn encode_string(s: &str) -> String {
    if s.is_ascii() {
        let mut r = String::from("(");
        for c in s.chars() {
            match c {
                '(' | ')' | '\\' => {
                    r.push('\\');
                    r.push(c);
                }
                c if (c as u32) < 32 || (c as u32) > 126 => r.push_str(&format!("\\{:03o}", c as u32)),
                c => r.push(c),
            }
        }
        r.push(')');
        return r;
    }
    let mut r = String::from("<FEFF");
    for u in s.encode_utf16() {
        r.push_str(&format!("{:04X}", u));
    }
    r.push('>');
    r
}

impl PdfPlotter {
    pub fn new(creation_date: &str) -> PdfPlotter {
        PdfPlotter {
            base: PlotterBase::new(),
            out: Vec::new(),
            work: None,
            xref: Vec::new(),
            stream_length_handle: 0,
            page_stream_handle: -1,
            page_tree_handle: 0,
            font_res_dict_handle: 0,
            img_res_dict_handle: 0,
            js_names_handle: 0,
            page_handles: Vec::new(),
            page_numbers: Vec::new(),
            page_name: String::new(),
            parent_page_name: String::new(),
            outline: Vec::new(),
            total_outline_nodes: 0,
            plot_scale_adj: (1.0, 1.0),
            creation_date: creation_date.to_string(),
        }
    }

    pub fn set_creator(&mut self, s: &str) {
        self.base.creator = s.to_string();
    }
    pub fn set_title(&mut self, s: &str) {
        self.base.title = s.to_string();
    }
    pub fn set_author(&mut self, s: &str) {
        self.base.author = s.to_string();
    }
    pub fn set_subject(&mut self, s: &str) {
        self.base.subject = s.to_string();
    }
    pub fn set_page_settings(&mut self, page: PageInfo) {
        self.base.page = page;
    }
    pub fn set_file_name(&mut self, name: &str) {
        self.base.filename = name.to_string();
    }

    /// `PDF_PLOTTER::SetViewport`.
    pub fn set_viewport(&mut self, offset: Pt, ius_per_decimil: f64, scale: f64, mirror: bool) {
        let b = &mut self.base;
        b.plot_mirror = mirror;
        b.plot_offset = offset;
        b.plot_scale = scale;
        b.ius_per_decimil = ius_per_decimil;
        // The CTM is set to 1 user unit per decimal
        b.iu_per_device_unit = 1.0 / ius_per_decimil;
    }

    fn w(&mut self, s: &str) {
        self.work.as_mut().expect("PDF content stream is open").extend_from_slice(s.as_bytes());
    }

    fn alloc_pdf_object(&mut self) -> usize {
        self.xref.push(0);
        self.xref.len() - 1
    }

    fn start_pdf_object(&mut self, handle: Option<usize>) -> usize {
        let h = handle.unwrap_or_else(|| self.alloc_pdf_object());
        self.xref[h] = self.out.len();
        let _ = writeln!(self.out, "{h} 0 obj");
        h
    }

    fn close_pdf_object(&mut self) {
        let _ = writeln!(self.out, "endobj");
    }

    fn start_pdf_stream(&mut self, handle: Option<usize>) -> usize {
        let h = self.start_pdf_object(handle);
        // This is guaranteed to be handle+1 but needs to be allocated since
        // you could allocate more object during stream preparation
        self.stream_length_handle = self.alloc_pdf_object();
        let _ = write!(self.out, "<< /Length {} 0 R >>\nstream\n", self.stream_length_handle);
        self.work = Some(Vec::new());
        h
    }

    fn close_pdf_stream(&mut self) {
        let data = self.work.take().expect("a stream is open");
        self.out.extend_from_slice(&data);
        let _ = write!(self.out, "\nendstream\n");
        self.close_pdf_object();
        // Writing the deferred length as an indirect object
        let lh = self.stream_length_handle;
        self.start_pdf_object(Some(lh));
        let _ = writeln!(self.out, "{}", data.len());
        self.close_pdf_object();
    }

    /// `PDF_PLOTTER::StartPlot( pageNumber, pageName )`.
    pub fn start_plot(&mut self, page_number: &str, page_name: &str) {
        // First things first: the customary null object
        self.xref.clear();
        self.xref.push(0);
        self.outline.clear();
        self.outline.push(OutlineNode { title: String::new(), entry_handle: -1, action_handle: -1, children: Vec::new() });
        self.total_outline_nodes = 0;
        // The header: the second line is binary junk (bit 7 set) so tools
        // treat the file as binary.
        self.out.extend_from_slice(b"%PDF-1.5\n%\x80\x81\x82\x83\n");
        // Allocate an entry for the page tree root, it will go in every page parent entry
        self.page_tree_handle = self.alloc_pdf_object();
        // In the same way, the font resource dictionary is used by every page
        self.font_res_dict_handle = self.alloc_pdf_object();
        self.img_res_dict_handle = self.alloc_pdf_object();
        self.js_names_handle = self.alloc_pdf_object();
        // Now, the PDF is read from the end, (more or less)... so we start
        // with the page stream for page 1.
        self.start_page(page_number, page_name, "", "");
    }

    /// `PDF_PLOTTER::StartPage`.
    pub fn start_page(&mut self, page_number: &str, page_name: &str, parent_page_number: &str, parent_page_name: &str) {
        self.page_numbers.push(page_number.to_string());
        self.page_name = if page_name.is_empty() { format!("Page {page_number}") } else { format!("{page_name} (Page {page_number})") };
        self.parent_page_name = if parent_page_name.is_empty() { format!("Page {parent_page_number}") } else { format!("{parent_page_name} (Page {parent_page_number})") };

        // Compute the paper size in IUs
        let page = self.base.page;
        self.base.paper_size = (page.width_mils * 10.0 / self.base.iu_per_device_unit, page.height_mils * 10.0 / self.base.iu_per_device_unit);
        // Set m_currentPenWidth to a unused value to ensure the pen width
        // will be initialized to a the right value in pdf file by the first item to plot
        self.base.current_pen_width = 0;

        // Open the content stream; the page object will go later
        self.page_stream_handle = self.start_pdf_stream(None) as i64;
        // Default graphic settings (coordinate system, default color and line style)
        let default_pen = self.base.user_to_device_size(ki_round(6.0 * IU_PER_MILS) as f64);
        let line = format!(
            "{} 0 0 {} 0 0 cm 1 J 1 j 0 0 0 rg 0 0 0 RG {} w\n",
            fmt_g(0.0072 * self.plot_scale_adj.0),
            fmt_g(0.0072 * self.plot_scale_adj.1),
            fmt_g(default_pen)
        );
        self.w(&line);
    }

    fn emit_goto_action(&mut self, page_handle: usize) -> usize {
        let h = self.alloc_pdf_object();
        self.start_pdf_object(Some(h));
        let _ = writeln!(self.out, "<</S /GoTo /D [{page_handle} 0 R /Fit]\n>>");
        self.close_pdf_object();
        h
    }

    fn add_outline_node(&mut self, parent: usize, action_handle: usize, title: &str) -> usize {
        let h = self.alloc_pdf_object() as i64;
        self.outline.push(OutlineNode { title: title.to_string(), entry_handle: h, action_handle: action_handle as i64, children: Vec::new() });
        let idx = self.outline.len() - 1;
        self.outline[parent].children.push(idx);
        self.total_outline_nodes += 1;
        idx
    }

    /// `PDF_PLOTTER::ClosePage`.
    pub fn close_page(&mut self) {
        if self.page_stream_handle != -1 {
            self.close_pdf_stream();
        }
        // Page size is in 1/72 of inch (default user space units).
        const PTS_PER_MIL: f64 = 0.072;
        let ps = (self.base.page.width_mils * PTS_PER_MIL, self.base.page.height_mils * PTS_PER_MIL);

        // Emit the page object and put it in the page list for later
        let page_handle = self.start_pdf_object(None);
        self.page_handles.push(page_handle);
        let _ = write!(
            self.out,
            "<<\n/Type /Page\n/Parent {} 0 R\n/Resources <<\n    /ProcSet [/PDF /Text /ImageC /ImageB]\n    /Font {} 0 R\n    /XObject {} 0 R >>\n/MediaBox [0 0 {} {}]\n",
            self.page_tree_handle,
            self.font_res_dict_handle,
            self.img_res_dict_handle,
            fmt_g(ps.0),
            fmt_g(ps.1)
        );
        if self.page_stream_handle != -1 {
            let _ = writeln!(self.out, "/Contents {} 0 R", self.page_stream_handle);
        }
        let _ = writeln!(self.out, ">>");
        self.close_pdf_object();

        // Mark the page stream as idle
        self.page_stream_handle = -1;

        let action = self.emit_goto_action(page_handle);
        let mut parent = 0usize;
        if !self.parent_page_name.is_empty() {
            // Search for the parent node iteratively through the entire tree
            let mut stack = vec![0usize];
            while let Some(n) = stack.pop() {
                if self.outline[n].title == self.parent_page_name {
                    parent = n;
                    break;
                }
                stack.extend(self.outline[n].children.iter().copied());
            }
        }
        let title = self.page_name.clone();
        self.add_outline_node(parent, action, &title);
    }

    fn emit_outline_node(&mut self, node: usize, parent_handle: i64, next: i64, prev: i64) {
        let node_handle = self.outline[node].entry_handle;
        let children = self.outline[node].children.clone();
        let mut prev_h = -1;
        for (i, &c) in children.iter().enumerate() {
            let next_h = if i + 1 >= children.len() { -1 } else { self.outline[children[i + 1]].entry_handle };
            self.emit_outline_node(c, node_handle, next_h, prev_h);
            prev_h = self.outline[c].entry_handle;
        }
        // -1 for parentHandle is the outline root itself which is handled elsewhere.
        if parent_handle != -1 {
            self.start_pdf_object(Some(node_handle as usize));
            let _ = write!(self.out, "<<\n/Title {}\n/Parent {} 0 R\n", encode_string(&self.outline[node].title), parent_handle);
            if next > 0 {
                let _ = writeln!(self.out, "/Next {next} 0 R");
            }
            if prev > 0 {
                let _ = writeln!(self.out, "/Prev {prev} 0 R");
            }
            if !children.is_empty() {
                let _ = writeln!(self.out, "/Count {}", -(children.len() as i64));
                let _ = writeln!(self.out, "/First {} 0 R", self.outline[children[0]].entry_handle);
                let _ = writeln!(self.out, "/Last {} 0 R", self.outline[*children.last().unwrap()].entry_handle);
            }
            if self.outline[node].action_handle != -1 {
                let _ = writeln!(self.out, "/A {} 0 R", self.outline[node].action_handle);
            }
            let _ = writeln!(self.out, ">>");
            self.close_pdf_object();
        }
    }

    fn emit_outline(&mut self) -> i64 {
        if self.outline[0].children.is_empty() {
            return -1;
        }
        // declare the outline object
        let h = self.alloc_pdf_object();
        self.outline[0].entry_handle = h as i64;
        self.emit_outline_node(0, -1, -1, -1);
        self.start_pdf_object(Some(h));
        let first = self.outline[self.outline[0].children[0]].entry_handle;
        let last = self.outline[*self.outline[0].children.last().unwrap()].entry_handle;
        let _ = write!(self.out, "<< /Type /Outlines\n   /Count {}\n   /First {} 0 R\n   /Last {} 0 R\n>>\n", self.total_outline_nodes, first, last);
        self.close_pdf_object();
        h as i64
    }

    /// `PDF_PLOTTER::EndPlot` -- returns the finished file.
    pub fn end_plot(&mut self) -> Vec<u8> {
        // Close the current page (often the only one)
        self.close_page();

        // `endPlotEmitResources`: this port emits no fonts and no images, so
        // the resource dictionaries are empty; `/Names` is an empty dictionary
        // (KiCad puts its popup-menu JavaScript there).
        let h = self.font_res_dict_handle;
        self.start_pdf_object(Some(h));
        let _ = writeln!(self.out, "<<");
        let _ = writeln!(self.out, ">>");
        self.close_pdf_object();
        let h = self.img_res_dict_handle;
        self.start_pdf_object(Some(h));
        let _ = writeln!(self.out, "<<\n");
        let _ = writeln!(self.out, ">>");
        self.close_pdf_object();
        let h = self.js_names_handle;
        self.start_pdf_object(Some(h));
        let _ = writeln!(self.out, "<< >>");
        self.close_pdf_object();

        // The page tree: it's a B-tree but luckily we only have few pages!
        let h = self.page_tree_handle;
        self.start_pdf_object(Some(h));
        let _ = write!(self.out, "<<\n/Type /Pages\n/Kids [\n");
        for i in 0..self.page_handles.len() {
            let _ = writeln!(self.out, "{} 0 R", self.page_handles[i]);
        }
        let _ = write!(self.out, "]\n/Count {}\n>>\n", self.page_handles.len());
        self.close_pdf_object();

        let info = self.start_pdf_object(None);
        if self.base.title.is_empty() {
            // Windows uses '\' and other platforms use '/' as separator
            self.base.title = self.base.filename.rsplit(['/', '\\']).next().unwrap_or("").to_string();
        }
        let _ = write!(
            self.out,
            "<<\n/Producer (KiCad PDF)\n/CreationDate ({})\n/Creator {}\n/Title {}\n/Author {}\n/Subject {}\n",
            self.creation_date,
            encode_string(&self.base.creator.clone()),
            encode_string(&self.base.title.clone()),
            encode_string(&self.base.author.clone()),
            encode_string(&self.base.subject.clone())
        );
        let _ = writeln!(self.out, ">>");
        self.close_pdf_object();

        // Let's dump in the outline
        let outline_handle = self.emit_outline();

        // The catalog, at last
        let catalog = self.start_pdf_object(None);
        if outline_handle > 0 {
            let _ = writeln!(
                self.out,
                "<<\n/Type /Catalog\n/Pages {} 0 R\n/Version /1.5\n/PageMode /UseOutlines\n/Outlines {} 0 R\n/Names {} 0 R\n/PageLayout /SinglePage\n>>",
                self.page_tree_handle, outline_handle, self.js_names_handle
            );
        } else {
            let _ = writeln!(self.out, "<<\n/Type /Catalog\n/Pages {} 0 R\n/Version /1.5\n/PageMode /UseNone\n/PageLayout /SinglePage\n>>", self.page_tree_handle);
        }
        self.close_pdf_object();

        // Emit the xref table (format is crucial to the byte, each entry
        // must be 20 bytes long, and object zero must be done in that way).
        let xref_start = self.out.len();
        let _ = write!(self.out, "xref\n0 {}\n0000000000 65535 f \n", self.xref.len());
        for i in 1..self.xref.len() {
            let _ = write!(self.out, "{:010} 00000 n \n", self.xref[i]);
        }
        // Done the xref, go for the trailer
        let _ = write!(self.out, "trailer\n<< /Size {} /Root {} 0 R /Info {} 0 R >>\nstartxref\n{}\n%%EOF\n", self.xref.len(), catalog, info, xref_start);
        std::mem::take(&mut self.out)
    }
}

impl Plotter for PdfPlotter {
    fn base(&self) -> &PlotterBase {
        &self.base
    }
    fn base_mut(&mut self) -> &mut PlotterBase {
        &mut self.base
    }

    fn set_current_line_width(&mut self, width: Iu) {
        // aWidth == 0 is promoted to 1 (PDF has no zero-width pen).
        let width = if width == 0 { 1 } else { width };
        if width != self.base.current_pen_width {
            let s = format!("{} w\n", fmt_g(self.base.user_to_device_size(width as f64)));
            self.w(&s);
        }
        self.base.current_pen_width = width;
    }

    /// `PSLIKE_PLOTTER::SetColor` + `PDF_PLOTTER::emitSetRGBColor`.
    fn set_color(&mut self, color: Color) {
        let (mut r, mut g, mut b, a) = if self.base.color_mode {
            (color.r, color.g, color.b, color.a)
        } else {
            let k = if color == Color::WHITE { 1.0 } else { 0.0 };
            (k, k, k, 1.0)
        };
        // PDF treats all colors as opaque, so blend with white paper.
        if a < 1.0 {
            r = r * a + (1.0 - a);
            g = g * a + (1.0 - a);
            b = b * a + (1.0 - a);
        }
        let s = format!("{} {} {} rg {} {} {} RG\n", fmt_g(r), fmt_g(g), fmt_g(b), fmt_g(r), fmt_g(g), fmt_g(b));
        self.w(&s);
    }

    fn set_dash(&mut self, line_width: Iu, style: LineStyle) {
        let dev = |s: &Self, v: f64| s.base.user_to_device_size(v) as i64;
        let s = match style {
            LineStyle::Dash => format!("[{} {}] 0 d\n", dev(self, dash_len(line_width)), dev(self, gap_len(line_width))),
            LineStyle::Dot => format!("[{} {}] 0 d\n", dev(self, dot_len(line_width)), dev(self, gap_len(line_width))),
            LineStyle::DashDot => format!("[{} {} {} {}] 0 d\n", dev(self, dash_len(line_width)), dev(self, gap_len(line_width)), dev(self, dot_len(line_width)), dev(self, gap_len(line_width))),
            LineStyle::DashDotDot => format!(
                "[{} {} {} {} {} {}] 0 d\n",
                dev(self, dash_len(line_width)),
                dev(self, gap_len(line_width)),
                dev(self, dot_len(line_width)),
                dev(self, gap_len(line_width)),
                dev(self, dot_len(line_width)),
                dev(self, gap_len(line_width))
            ),
            _ => "[] 0 d\n\n".to_string(),
        };
        self.w(&s);
    }

    fn rect(&mut self, p1: Pt, p2: Pt, fill: FillT, width: Iu) {
        if fill == FillT::NoFill && width == 0 {
            return;
        }
        self.set_current_line_width(width);
        let size = (p2.x - p1.x, p2.y - p1.y);
        if size.0 == 0 && size.1 == 0 {
            // Can't draw zero-sized rectangles
            self.move_to(p1);
            self.finish_to(p1);
            return;
        }
        if size.0.abs().min(size.1.abs()) < width {
            // Too thick stroked rectangles are buggy, draw as polygon
            let pts = [Pt::new(p1.x, p1.y), Pt::new(p2.x, p1.y), Pt::new(p2.x, p2.y), Pt::new(p1.x, p2.y), Pt::new(p1.x, p1.y)];
            self.plot_poly(&pts, fill, width);
            return;
        }
        let a = self.base.user_to_device(p1);
        let b = self.base.user_to_device(p2);
        let paint = if fill == FillT::NoFill {
            'S'
        } else if width > 0 {
            'B'
        } else {
            'f'
        };
        let s = format!("{} {} {} {} re {}\n", fmt_g(a.0), fmt_g(a.1), fmt_g(b.0 - a.0), fmt_g(b.1 - a.1), paint);
        self.w(&s);
    }

    fn circle(&mut self, pos: Pt, diameter: Iu, fill: FillT, width: Iu) {
        if fill == FillT::NoFill && width == 0 {
            return;
        }
        self.set_current_line_width(width);
        let pos_dev = self.base.user_to_device(pos);
        let mut radius = self.base.user_to_device_size(diameter as f64 / 2.0);
        let mut fill = fill;
        // If diameter is less than width, switch to filled mode
        if fill == FillT::NoFill && diameter < self.current_line_width() {
            fill = FillT::FilledShape;
            radius = self.base.user_to_device_size(diameter as f64 / 2.0 + width as f64 / 2.0);
        }
        // PDF doesn't support circles: four cubic beziers with the
        // well-known 0.551784 control-point ratio.
        let magic = radius * 0.551784;
        let (x, y) = pos_dev;
        let g = fmt_g;
        let s = format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {}\n",
            g(x - radius),
            g(y),
            g(x - radius),
            g(y + magic),
            g(x - magic),
            g(y + radius),
            g(x),
            g(y + radius),
            g(x + magic),
            g(y + radius),
            g(x + radius),
            g(y + magic),
            g(x + radius),
            g(y),
            g(x + radius),
            g(y - magic),
            g(x + magic),
            g(y - radius),
            g(x),
            g(y - radius),
            g(x - magic),
            g(y - radius),
            g(x - radius),
            g(y - magic),
            g(x - radius),
            g(y),
            if fill == FillT::NoFill { 's' } else { 'b' }
        );
        self.w(&s);
    }

    fn arc(&mut self, center: (f64, f64), start_angle_deg: f64, angle_deg: f64, radius: f64, fill: FillT, width: Iu) {
        self.set_current_line_width(width);
        if radius <= 0.0 {
            let w = self.current_line_width();
            self.circle(Pt::new(ki_round(center.0), ki_round(center.1)), w, FillT::FilledShape, 0);
            return;
        }
        // `arcPath`: approximated "in the old way", 5 degree steps.
        let mut start_a = -start_angle_deg;
        let mut end_a = start_a - angle_deg;
        if start_a > end_a {
            std::mem::swap(&mut start_a, &mut end_a);
        }
        let at = |a: f64, s: &Self| {
            let p = Pt::new(ki_round(center.0 + radius * (-a).to_radians().cos()), ki_round(center.1 + radius * (-a).to_radians().sin()));
            s.base.user_to_device(p)
        };
        let mut path = vec![at(start_a, self)];
        let mut ii = start_a + 5.0;
        while ii < end_a {
            path.push(at(ii, self));
            ii += 5.0;
        }
        path.push(at(end_a, self));
        let mut s = String::new();
        if path.len() >= 2 {
            s += &format!("{} {} m ", fmt_g(path[0].0), fmt_g(path[0].1));
            for p in &path[1..] {
                s += &format!("{} {} l ", fmt_g(p.0), fmt_g(p.1));
            }
        }
        if fill == FillT::NoFill {
            s += "S\n";
        } else {
            let c = self.base.user_to_device(Pt::new(ki_round(center.0), ki_round(center.1)));
            s += &format!("{} {} l b\n", fmt_g(c.0), fmt_g(c.1));
        }
        self.w(&s);
    }

    fn plot_poly(&mut self, pts: &[Pt], fill: FillT, width: Iu) {
        if pts.len() <= 1 {
            return;
        }
        if fill == FillT::NoFill && width == 0 {
            return;
        }
        self.set_current_line_width(width);
        let p0 = self.base.user_to_device(pts[0]);
        let mut s = format!("{:.6} {:.6} m ", p0.0, p0.1);
        for p in &pts[1..] {
            let d = self.base.user_to_device(*p);
            s += &format!("{:.6} {:.6} l ", d.0, d.1);
        }
        // Close path and stroke and/or fill
        if fill == FillT::NoFill {
            s += "S\n";
        } else if width == 0 {
            s += "h f\n";
        } else {
            s += "b\n";
        }
        self.w(&s);
    }

    fn pen_to(&mut self, pos: Pt, plume: char) {
        if plume == 'Z' {
            if self.base.pen_state != 'Z' {
                self.w("S\n");
                self.base.pen_state = 'Z';
                self.base.pen_last_pos = Pt::new(-1, -1);
            }
            return;
        }
        if self.base.pen_state != plume || pos != self.base.pen_last_pos {
            let d = self.base.user_to_device(pos);
            let s = format!("{:.6} {:.6} {}\n", d.0, d.1, if plume == 'D' { 'l' } else { 'm' });
            self.w(&s);
        }
        self.base.pen_state = plume;
        self.base.pen_last_pos = pos;
    }

    /// `PDF_PLOTTER::Text` rejects zero-sized text ("PDF files do not like 0
    /// sized texts which create broken files").
    fn text(&mut self, pos: Pt, color: Color, text: &str, attrs: &TextAttrs) {
        if attrs.size.0 == 0 || attrs.size.1 == 0 {
            return;
        }
        self.set_current_line_width(attrs.pen_width);
        self.stroke_text(pos, color, text, attrs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_encoding() {
        assert_eq!(encode_string("a(b)\\"), "(a\\(b\\)\\\\)");
        assert_eq!(encode_string("é"), "<FEFF00E9>");
    }

    #[test]
    fn minimal_pdf_is_wellformed() {
        let mut p = PdfPlotter::new("D:2026:01:01:00:00:00");
        p.set_color_mode(true);
        p.set_page_settings(PageInfo::A4);
        p.set_viewport(Pt::default(), IUS_PER_DECIMIL, 1.0, false);
        p.set_creator("test");
        p.start_plot("1", "root");
        p.set_color(Color::BLACK);
        p.move_to(Pt::new(0, 0));
        p.finish_to(Pt::new(1000, 1000));
        let bytes = p.end_plot();
        assert!(bytes.starts_with(b"%PDF-1.5\n"));
        assert!(bytes.ends_with(b"%%EOF\n"));
    }
}
