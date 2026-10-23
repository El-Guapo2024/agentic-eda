//! ngspice's raw files (`write` in a control block, `-r` on the command line), binary and ASCII, as ngspice 42 writes them
//! (`src/frontend/rawfile.c`, `raw_write`): one or more plots, each a header (`Title:`, `Date:`, `Plotname:`, `Flags:`, `No. Variables:`,
//! `No. Points:`, `Variables:` and a line per vector) and the points, point by point, every vector's value in turn -- eight bytes of
//! little-endian double, or two for a complex plot, after `Binary:`; `index<TAB>value` lines, a complex value written `re,im`, after `Values:`.
//!
//! KiCad reads its vectors from libngspice's memory instead (`NGSPICE::GetRealVector`, `GetGainVector`, `GetPhaseVector`); a batch run hands
//! them over in this file, and these are the same numbers.

/// One vector of a plot.
#[derive(Debug, Clone, PartialEq)]
pub struct RawVector {
    pub name: String,
    /// The type ngspice gives it: `time`, `frequency`, `voltage`, `current`, `notype`, ...
    pub kind: String,
    pub real: Vec<f64>,
    /// The imaginary parts, for a complex plot.
    pub imag: Option<Vec<f64>>,
}

impl RawVector {
    /// `NGSPICE::GetGainVector`: the real values, or the magnitude of complex ones.
    pub fn gain(&self) -> Vec<f64> {
        match &self.imag {
            None => self.real.clone(),
            Some(im) => self.real.iter().zip(im).map(|(r, i)| r.hypot(*i)).collect(),
        }
    }

    /// `NGSPICE::GetPhaseVector`: atan2 of complex values in radians; zero for real ones ("well, that's life").
    pub fn phase(&self) -> Vec<f64> {
        match &self.imag {
            None => vec![0.0; self.real.len()],
            Some(im) => self.real.iter().zip(im).map(|(r, i)| i.atan2(*r)).collect(),
        }
    }
}

/// One plot (`tran1`, `ac1`, `op1`, ...) of a raw file.
#[derive(Debug, Clone, PartialEq)]
pub struct RawPlot {
    pub title: String,
    pub date: String,
    /// `Plotname:`, e.g. "Transient Analysis".
    pub plot_name: String,
    pub complex: bool,
    pub points: usize,
    pub vectors: Vec<RawVector>,
}

impl RawPlot {
    /// A vector by name, matched without regard to case as ngspice matches them.
    pub fn vector(&self, name: &str) -> Option<&RawVector> {
        self.vectors.iter().find(|v| v.name.eq_ignore_ascii_case(name))
    }
}

fn line_at(data: &[u8], pos: &mut usize) -> Option<String> {
    if *pos >= data.len() {
        return None;
    }
    let start = *pos;
    while *pos < data.len() && data[*pos] != b'\n' {
        *pos += 1;
    }
    let line = String::from_utf8_lossy(&data[start..*pos]).trim_end_matches('\r').to_string();
    if *pos < data.len() {
        *pos += 1;
    }
    Some(line)
}

fn number(s: &str) -> Result<f64, String> {
    s.trim().parse::<f64>().map_err(|_| format!("not a number in the raw file: {s:?}"))
}

/// Every plot of a raw file, binary or ASCII (a file may hold both, one plot after another).
pub fn parse_raw(data: &[u8]) -> Result<Vec<RawPlot>, String> {
    let mut plots = Vec::new();
    let mut pos = 0usize;
    loop {
        // Skip blank lines between plots.
        let mut header = None;
        while let Some(l) = line_at(data, &mut pos) {
            if !l.trim().is_empty() {
                header = Some(l);
                break;
            }
        }
        let Some(first) = header else { break };
        let mut plot = RawPlot { title: String::new(), date: String::new(), plot_name: String::new(), complex: false, points: 0, vectors: Vec::new() };
        let mut nvars = 0usize;
        let mut line = Some(first);
        let mut binary = None;
        while let Some(l) = line.take() {
            let (key, val) = l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).unwrap_or((l.trim().to_string(), String::new()));
            match key.to_lowercase().as_str() {
                "title" => plot.title = val,
                "date" => plot.date = val,
                "plotname" => plot.plot_name = val,
                "flags" => plot.complex = val.split_whitespace().any(|f| f.eq_ignore_ascii_case("complex")),
                "no. variables" => nvars = val.parse().map_err(|_| format!("bad variable count {val:?}"))?,
                "no. points" => plot.points = val.parse().map_err(|_| format!("bad point count {val:?}"))?,
                "variables" => {
                    for _ in 0..nvars {
                        let v = line_at(data, &mut pos).ok_or("the raw file ends in its variables")?;
                        let mut f = v.split(['\t', ' ']).filter(|s| !s.is_empty());
                        let _index = f.next();
                        let name = f.next().ok_or_else(|| format!("a variable without a name: {v:?}"))?.to_string();
                        let kind = f.next().unwrap_or("notype").to_string();
                        plot.vectors.push(RawVector { name, kind, real: Vec::with_capacity(plot.points), imag: plot.complex.then(|| Vec::with_capacity(plot.points)) });
                    }
                }
                "binary" => binary = Some(true),
                "values" => binary = Some(false),
                // "Command:", "Option:", "Dimensions:" and anything else ngspice adds.
                _ => {}
            }
            if binary.is_some() {
                break;
            }
            line = line_at(data, &mut pos);
        }
        let Some(binary) = binary else { return Err("a raw file plot without Binary: or Values:".into()) };
        if plot.vectors.len() != nvars {
            return Err(format!("the raw file says {nvars} variables and lists {}", plot.vectors.len()));
        }
        if binary {
            let width = if plot.complex { 16 } else { 8 };
            let need = plot.points * nvars * width;
            if data.len() < pos + need {
                return Err(format!("the raw file's binary data is short: {} of {need} bytes", data.len() - pos));
            }
            let f = |at: usize| f64::from_le_bytes(data[at..at + 8].try_into().unwrap_or([0; 8]));
            for p in 0..plot.points {
                for (k, v) in plot.vectors.iter_mut().enumerate() {
                    let at = pos + (p * nvars + k) * width;
                    v.real.push(f(at));
                    if let Some(im) = v.imag.as_mut() {
                        im.push(f(at + 8));
                    }
                }
            }
            pos += need;
        } else {
            for _ in 0..plot.points {
                for k in 0..nvars {
                    let l = loop {
                        let l = line_at(data, &mut pos).ok_or("the raw file ends in its values")?;
                        if !l.trim().is_empty() {
                            break l;
                        }
                    };
                    // The first vector's line starts with the point's index.
                    let text = if k == 0 { l.split_whitespace().nth(1).unwrap_or("").to_string() } else { l.trim().to_string() };
                    let v = &mut plot.vectors[k];
                    if plot.complex {
                        let (re, im) = text.split_once(',').ok_or_else(|| format!("a complex value without its imaginary part: {text:?}"))?;
                        v.real.push(number(re)?);
                        if let Some(i) = v.imag.as_mut() {
                            i.push(number(im)?);
                        }
                    } else {
                        v.real.push(number(&text)?);
                    }
                }
            }
        }
        plots.push(plot);
    }
    Ok(plots)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binary_plot(name: &str, vars: &[(&str, &str)], points: &[Vec<f64>], complex: bool) -> Vec<u8> {
        let mut s = format!("Title: test\nDate: today\nPlotname: {name}\nFlags: {}\nNo. Variables: {}\nNo. Points: {}\nVariables:\n", if complex { "complex" } else { "real" }, vars.len(), points.len());
        for (i, (n, k)) in vars.iter().enumerate() {
            s.push_str(&format!("\t{i}\t{n}\t{k}\n"));
        }
        s.push_str("Binary:\n");
        let mut b = s.into_bytes();
        for p in points {
            for x in p {
                b.extend_from_slice(&x.to_le_bytes());
            }
        }
        b
    }

    #[test]
    fn a_binary_real_plot_is_read_point_by_point() {
        let data = binary_plot("Transient Analysis", &[("time", "time"), ("v(out)", "voltage")], &[vec![0.0, 0.0], vec![1e-3, 3.16]], false);
        let plots = parse_raw(&data).unwrap();
        assert_eq!(plots.len(), 1);
        let p = &plots[0];
        assert_eq!(p.plot_name, "Transient Analysis");
        assert_eq!(p.vector("V(OUT)").unwrap().real, vec![0.0, 3.16]);
        assert_eq!(p.vector("time").unwrap().kind, "time");
    }

    #[test]
    fn a_complex_plot_gives_gain_and_phase() {
        let data = binary_plot("AC Analysis", &[("frequency", "frequency"), ("v(out)", "voltage")], &[vec![10.0, 0.0, 0.0, 1.0]], true);
        let p = &parse_raw(&data).unwrap()[0];
        let v = p.vector("v(out)").unwrap();
        assert_eq!(v.gain(), vec![1.0]);
        assert!((v.phase()[0] - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
        assert_eq!(p.vector("frequency").unwrap().real, vec![10.0]);
    }

    #[test]
    fn ascii_plots_follow_one_another() {
        let text = "Title: t\nDate: d\nPlotname: Noise Spectral Density Curves\nFlags: real\nNo. Variables: 2\nNo. Points: 2\nVariables:\n\t0\tfrequency\tfrequency grid=3\n\t1\tonoise_spectrum\tnotype\nValues:\n 0\t1.000000000000000e+00\n\t2.5e-09\n\n 1\t1.0e+01\n\t2.4e-09\n\nTitle: t\nDate: d\nPlotname: AC Analysis\nFlags: complex\nNo. Variables: 1\nNo. Points: 1\nVariables:\n\t0\tfrequency\tfrequency\nValues:\n 0\t1.0e+00,0.0e+00\n";
        let plots = parse_raw(text.as_bytes()).unwrap();
        assert_eq!(plots.len(), 2);
        assert_eq!(plots[0].vector("onoise_spectrum").unwrap().real, vec![2.5e-9, 2.4e-9]);
        assert!(plots[1].complex);
        assert_eq!(plots[1].vectors[0].imag.as_deref(), Some(&[0.0][..]));
    }

    #[test]
    fn a_short_binary_file_is_an_error() {
        let mut data = binary_plot("x", &[("time", "time")], &[vec![1.0], vec![2.0]], false);
        data.truncate(data.len() - 3);
        assert!(parse_raw(&data).unwrap_err().contains("short"));
    }
}
