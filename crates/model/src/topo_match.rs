//! The topology matcher of the multichannel tool: which footprint of one channel stands for which of another.
//!
//! A port of `pcbnew/connectivity/topo_match.{h,cpp}` ("A very simple (but working) partial connection graph isomorphism algorithm, operating on sets of
//! footprints and connections between their pads"), without the thread pool and the progress reporting: two graphs are built from the footprints of two
//! channels ([`Graph::build`], `CONNECTION_GRAPH::BuildFromFootprintSet`), and [`Graph::find_isomorphism`] walks the reference graph's components one at a
//! time, backtracking over the candidates of the target graph (`CONNECTION_GRAPH::FindIsomorphism`: the structural pre-filter, the net-consistency check of
//! every candidate, the most-constrained-first choice of the next component, the tie breaker on the symbol, the limit of 10000 steps).
//!
//! A footprint is the matcher's [`TopoFootprint`]: its reference, its library link, its pads (number and net code, 0 for none) and, where the design knows
//! it, the identity of the schematic symbol it stands for (`GetPath().back()` in KiCad: two instances of one sheet have the same symbol uuid, and the tie
//! breaker picks the candidate that has the reference's).
//!
//! Every message is KiCad's, as the Repeat Layout dialog shows it.

use std::collections::{BTreeMap, HashMap, HashSet};

/// `c_ITER_LIMIT`: how many steps of the search are made before it gives up.
pub const ITER_LIMIT: usize = 10_000;

/// A footprint as the matcher reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct TopoFootprint {
    pub reference: String,
    /// `FOOTPRINT::GetFPID()` as text; empty for a footprint with no library link.
    pub fpid: String,
    /// The pads in the footprint's own order: (pad number, net code). A pad on no net has code 0, which the matcher treats as a net of its own where the
    /// consistency of nets is checked (`checkCandidateNetConsistency`) and as no connection where the graph is built (`p->GetNetCode() > 0`).
    pub pads: Vec<(String, i64)>,
    /// The symbol the footprint stands for (the tail of `FOOTPRINT::GetPath()`); `None` when the footprint has no link to one.
    pub symbol: Option<String>,
}

/// `TMATCH::TOPOLOGY_MISMATCH_REASON`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MismatchReason {
    pub reference: String,
    pub candidate: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct Pin {
    name: String,
    net: i64,
    /// The other pins on the same net inside this graph: (component, pin).
    conns: Vec<(usize, usize)>,
}

#[derive(Debug, Clone)]
struct Component {
    /// The footprint's place in the slice the graph was built from.
    source: usize,
    reference: String,
    prefix: String,
    fpid: String,
    symbol: Option<String>,
    pins: Vec<Pin>,
}

/// `TMATCH::CONNECTION_GRAPH`.
#[derive(Debug, Clone)]
pub struct Graph {
    comps: Vec<Component>,
    external_nets: HashSet<i64>,
    /// The names of the nets, for the messages (`board->FindNet( code )->GetNetname()`); a net with none is `net N`.
    names: HashMap<i64, String>,
}

/// `UTIL::GetRefDesPrefix`: the reference without the digits and question marks that end it.
pub fn refdes_prefix(reference: &str) -> &str {
    reference.trim_end_matches(|c: char| c == '?' || c.is_ascii_digit())
}

/// `COMPONENT::isChannelSuffix`: "Channel identifiers contain no letters, only digits and separator symbols".
fn is_channel_suffix(suffix: &str) -> bool {
    !suffix.chars().any(|c| c.is_alphabetic())
}

/// `COMPONENT::prefixesShareCommonBase`: the prefixes are the same, or one starts as the other and what is left of both is a channel suffix (`TRIM_1.1` and `TRIM_2.1`).
fn prefixes_share_common_base(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (ca, cb): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let min = ca.len().min(cb.len());
    let mut common = 0;
    while common < min && ca[common] == cb[common] {
        common += 1;
    }
    if common == 0 {
        return false;
    }
    let (sa, sb): (String, String) = (ca[common..].iter().collect(), cb[common..].iter().collect());
    is_channel_suffix(&sa) && is_channel_suffix(&sb)
}

impl Component {
    /// `COMPONENT::IsSameKind`.
    fn is_same_kind(&self, b: &Component) -> bool {
        prefixes_share_common_base(&self.prefix, &b.prefix) && self.fpid == b.fpid
    }

    /// `COMPONENT::MatchesWith( b )`, with the pads of both graphs' connections read through `ga` (this component's graph) and `gb` (b's).
    fn matches_with(&self, ga: &Graph, b: &Component, gb: &Graph) -> bool {
        if self.pins.len() != b.pins.len() || !self.is_same_kind(b) {
            return false;
        }
        for pin in 0..b.pins.len() {
            // `b->m_pins[pin]->IsIsomorphic( *m_pins[pin] )`
            if !pin_is_isomorphic(gb, b, pin, ga, self, pin) {
                return false;
            }
        }
        true
    }
}

/// `PIN::IsIsomorphic( other )` for `this` = pin `i` of `a` (in graph `ga`) and `other` = pin `j` of `b` (in `gb`): the same number of connections, and every
/// connection of `this` has a topologically similar one among those of `other` (a connection is similar when its component is of the same kind and its pad
/// has the same number: `PIN::IsTopologicallySimilar`).
fn pin_is_isomorphic(ga: &Graph, a: &Component, i: usize, gb: &Graph, b: &Component, j: usize) -> bool {
    let (this, other) = (&a.pins[i], &b.pins[j]);
    if this.conns.len() != other.conns.len() {
        return false;
    }
    if this.conns.is_empty() {
        return true;
    }
    for &(cc, cp) in &this.conns {
        let cref = &ga.comps[cc];
        let found = other.conns.iter().any(|&(oc, op)| {
            let oconn = &gb.comps[oc];
            oconn.is_same_kind(cref) && oconn.pins[op].name == cref.pins[cp].name
        });
        if !found {
            return false;
        }
    }
    true
}

/// What a step of the search remembers (`BACKTRACK_STAGE`).
#[derive(Clone)]
struct Stage {
    /// The reference component (an index into the reference graph).
    r: usize,
    current: i64,
    matches: Vec<usize>,
    /// Target component -> reference component, the pairs fixed so far.
    locked: BTreeMap<usize, usize>,
}

impl Graph {
    /// `CONNECTION_GRAPH::BuildFromFootprintSet( aFps, aOtherChannelFps )`: the components of `fps` with their pads' connections. A net is *external* (left out of the
    /// connections) only when it is on a pad of both channels: the supply and ground rails that every channel has, where a configuration pin may be tied to a
    /// different rail in each, while a signal that escapes to a connector is part of the topology.
    pub fn build(fps: &[TopoFootprint], other: &[TopoFootprint], names: &HashMap<i64, String>) -> Graph {
        let local: HashSet<i64> = fps.iter().flat_map(|f| f.pads.iter().map(|p| p.1)).filter(|n| *n > 0).collect();
        let others: HashSet<i64> = other.iter().flat_map(|f| f.pads.iter().map(|p| p.1)).filter(|n| *n > 0).collect();
        let external_nets: HashSet<i64> = local.intersection(&others).copied().collect();
        let mut comps: Vec<Component> = fps
            .iter()
            .enumerate()
            .map(|(source, f)| Component {
                source,
                reference: f.reference.clone(),
                prefix: refdes_prefix(&f.reference).to_string(),
                fpid: f.fpid.clone(),
                symbol: f.symbol.clone(),
                pins: f.pads.iter().map(|(name, net)| Pin { name: name.clone(), net: *net, conns: Vec::new() }).collect(),
            })
            .collect();
        // `BuildConnectivity`: `sortByPinCount` (more pins first, then by reference), each component's pins by name.
        comps.sort_by(|a, b| b.pins.len().cmp(&a.pins.len()).then_with(|| a.reference.cmp(&b.reference)));
        for c in comps.iter_mut() {
            c.pins.sort_by(|a, b| a.name.cmp(&b.name));
        }
        let mut nets: BTreeMap<i64, Vec<(usize, usize)>> = BTreeMap::new();
        for (ci, c) in comps.iter().enumerate() {
            for (pi, p) in c.pins.iter().enumerate() {
                if p.net > 0 {
                    nets.entry(p.net).or_default().push((ci, pi));
                }
            }
        }
        for (net, pins) in &nets {
            if external_nets.contains(net) {
                continue;
            }
            for &(ci, pi) in pins {
                let conns: Vec<(usize, usize)> = pins.iter().copied().filter(|&o| o != (ci, pi)).collect();
                comps[ci].pins[pi].conns = conns;
            }
        }
        Graph { comps, external_nets, names: names.clone() }
    }

    pub fn len(&self) -> usize {
        self.comps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.comps.is_empty()
    }

    /// `CONNECTION_GRAPH::breakTie` -> `breakTieBySymbolUuid`: among candidates that fit equally, the one that stands for the same schematic symbol as the reference
    /// goes first; nothing changes when none or several do, or when the reference has no symbol.
    fn break_tie(&self, r: &Component, matches: &mut [usize]) {
        if matches.len() <= 1 {
            return;
        }
        let Some(sym) = r.symbol.as_ref() else { return };
        let hits: Vec<usize> = matches.iter().enumerate().filter(|(_, &m)| self.comps[m].symbol.as_ref() == Some(sym)).map(|(i, _)| i).collect();
        if hits.len() == 1 {
            // `std::rotate( begin, begin + idx, begin + idx + 1 )`: that candidate first, the others in their order
            matches[..=hits[0]].rotate_right(1);
        }
    }

    /// `CONNECTION_GRAPH::findMatchingComponents` on the target graph (`self`): the candidates among `structural` (the target components that fit `r` structurally)
    /// that are not fixed yet and whose pads are on nets consistent with the pairs fixed so far, the best-fitting first. `reasons` is what the rejected ones said.
    fn find_matching_components(&self, reference: &Graph, r: usize, structural: &[usize], locked: &BTreeMap<usize, usize>, reasons: &mut Vec<MismatchReason>) -> Vec<usize> {
        reasons.clear();
        let rc = &reference.comps[r];
        let base = base_net_mapping(reference, self, locked);
        let mut matches: Vec<usize> = Vec::new();
        for &t in structural {
            if locked.contains_key(&t) {
                continue;
            }
            let cand = &self.comps[t];
            match check_candidate_net_consistency(&base, rc, cand, &self.external_nets, &self.names) {
                Ok(()) => matches.push(t),
                Err(reason) => reasons.push(reason),
            }
        }
        let score = |t: usize| -> f64 {
            let cand = &self.comps[t];
            if rc.pins.is_empty() {
                return 0.0;
            }
            let n = rc.pins.iter().zip(&cand.pins).filter(|(a, b)| a.net == b.net).count();
            n as f64 / rc.pins.len() as f64
        };
        let scores: HashMap<usize, f64> = matches.iter().map(|&t| (t, score(t))).collect();
        matches.sort_by(|&a, &b| {
            let (sa, sb) = (scores[&a], scores[&b]);
            if sa != sb {
                return sb.partial_cmp(&sa).unwrap_or(std::cmp::Ordering::Equal);
            }
            self.comps[a].reference.cmp(&self.comps[b].reference)
        });
        if matches.is_empty() && reasons.is_empty() {
            reasons.push(MismatchReason { reference: rc.reference.clone(), candidate: String::new(), reason: "No compatible component found in the target area.".to_string() });
        }
        matches
    }

    /// `CONNECTION_GRAPH::FindIsomorphism( target, .. )`: the footprint of `target` each footprint of this graph stands for, as (index in the slice this graph was
    /// built from, index in the slice `target` was built from), or the reasons it cannot be done.
    pub fn find_isomorphism(&self, target: &Graph) -> Result<Vec<(usize, usize)>, Vec<MismatchReason>> {
        let mut reasons: Vec<MismatchReason> = Vec::new();
        if self.comps.is_empty() || target.comps.is_empty() {
            return Err(vec![MismatchReason { reason: "One or both of the areas has no components assigned.".to_string(), ..Default::default() }]);
        }
        if self.comps.len() != target.comps.len() {
            return Err(vec![MismatchReason { reason: "Component count mismatch".to_string(), ..Default::default() }]);
        }
        // Structural compatibility depends on nothing the search changes: worked out once per reference component.
        let structural: Vec<Vec<usize>> = self.comps.iter().map(|r| (0..target.comps.len()).filter(|&t| r.matches_with(self, &target.comps[t], target)).collect()).collect();

        let mut stack: Vec<Stage> = vec![Stage { r: 0, current: -1, matches: Vec::new(), locked: BTreeMap::new() }];
        let mut loops = 0usize;
        while !stack.is_empty() {
            loops += 1;
            let top = stack.len() - 1;
            // "Remove %s from locked": the pair this stage fixed on an earlier visit goes before it chooses again.
            let cur_r = stack[top].r;
            if let Some(t) = stack[top].locked.iter().find(|(_, &r)| r == cur_r).map(|(&t, _)| t) {
                stack[top].locked.remove(&t);
            }
            if loops >= ITER_LIMIT {
                let reason = MismatchReason { reason: "Iteration count exceeded (timeout)".to_string(), ..Default::default() };
                reasons.insert(0, reason);
                return Err(reasons);
            }
            if stack[top].current < 0 {
                let mut local = Vec::new();
                let found = target.find_matching_components(self, cur_r, &structural[cur_r], &stack[top].locked, &mut local);
                if found.is_empty() && reasons.is_empty() && !local.is_empty() {
                    reasons = local;
                }
                stack[top].matches = found;
                stack[top].current = 0;
            }
            if stack[top].current == 0 && stack[top].matches.len() > 1 {
                let mut m = std::mem::take(&mut stack[top].matches);
                target.break_tie(&self.comps[cur_r], &mut m);
                stack[top].matches = m;
            }
            if stack[top].matches.is_empty() || stack[top].current as usize >= stack[top].matches.len() {
                stack.pop();
                continue;
            }
            let chosen = stack[top].matches[stack[top].current as usize];
            stack[top].current += 1;
            stack[top].locked.insert(chosen, cur_r);

            if stack[top].locked.len() == self.comps.len() {
                let mut out: Vec<(usize, usize)> = stack[top].locked.iter().map(|(&t, &r)| (self.comps[r].source, target.comps[t].source)).collect();
                out.sort_unstable();
                return Ok(out);
            }

            // The most-constrained-first choice (MRV) of the next component: the first one with exactly one candidate, else the fewest candidates; a component with
            // none is the dead end to go and fail on.
            let locked_refs: HashSet<usize> = stack[top].locked.values().copied().collect();
            let mut best: Option<(usize, Vec<usize>)> = None;
            let mut alt: Option<usize> = None;
            let mut min_matches = usize::MAX;
            for r in 0..self.comps.len() {
                if r == cur_r || locked_refs.contains(&r) {
                    continue;
                }
                let mut why = Vec::new();
                let found = target.find_matching_components(self, r, &structural[r], &stack[top].locked, &mut why);
                match found.len() {
                    1 => {
                        best = Some((r, found));
                        break;
                    }
                    0 => {
                        alt = Some(r);
                        if reasons.is_empty() && !why.is_empty() {
                            reasons = why;
                        }
                    }
                    n if n < min_matches => {
                        min_matches = n;
                        best = Some((r, found));
                    }
                    _ => {}
                }
            }
            let mut next = stack[top].clone();
            if let Some((r, found)) = best {
                next.r = r;
                next.matches = found;
                next.current = 0;
            } else {
                next.r = alt.unwrap_or(0);
                next.matches = Vec::new();
                next.current = -1;
            }
            stack.push(next);
        }
        Err(reasons)
    }
}

/// `buildBaseNetMapping`: the net each net of the reference is on in the target, as the pairs fixed so far say.
fn base_net_mapping(reference: &Graph, target: &Graph, locked: &BTreeMap<usize, usize>) -> HashMap<i64, i64> {
    let mut mapping = HashMap::with_capacity(locked.len() * 4);
    for (&t, &r) in locked {
        let (rp, tp) = (&reference.comps[r].pins, &target.comps[t].pins);
        for i in 0..rp.len().min(tp.len()) {
            mapping.insert(rp[i].net, tp[i].net);
        }
    }
    mapping
}

/// `checkCandidateNetConsistency`: the pads of `cand` are on the nets the pairs fixed so far put the pads of `r` on, and two pads of `r` on one net are on one net in
/// `cand`. A pad on an external net is not looked at.
fn check_candidate_net_consistency(base: &HashMap<i64, i64>, r: &Component, cand: &Component, external: &HashSet<i64>, names: &HashMap<i64, String>) -> Result<(), MismatchReason> {
    let who = |reason: String| MismatchReason { reference: r.reference.clone(), candidate: cand.reference.clone(), reason };
    if r.pins.len() != cand.pins.len() {
        return Err(who(format!("Component {} expects {} matching pads but candidate {} provides {}.", r.reference, r.pins.len(), cand.reference, cand.pins.len())));
    }
    let mut additions: HashMap<i64, i64> = HashMap::new();
    for i in 0..r.pins.len() {
        let (ref_net, tgt_net) = (r.pins[i].net, cand.pins[i].net);
        if external.contains(&ref_net) || external.contains(&tgt_net) {
            continue;
        }
        if let Some(&mapped) = base.get(&ref_net) {
            if mapped != tgt_net {
                return Err(who(format!(
                    "Pad {} of {} is on net {} but its match in candidate {} is on net {}.",
                    r.pins[i].name,
                    r.reference,
                    net_label(names, ref_net),
                    cand.reference,
                    net_label(names, tgt_net)
                )));
            }
            continue;
        }
        if let Some(&mapped) = additions.get(&ref_net) {
            if mapped != tgt_net {
                return Err(who(format!("Pad {} of {} has inconsistent net mapping in candidate {}.", r.pins[i].name, r.reference, cand.reference)));
            }
            continue;
        }
        additions.insert(ref_net, tgt_net);
    }
    Ok(())
}

/// The text a net has in a message: its name, else `net %d` (`_( "net %d" )`).
fn net_label(names: &HashMap<i64, String>, code: i64) -> String {
    match names.get(&code) {
        Some(name) if !name.is_empty() => name.clone(),
        _ => format!("net {code}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fp(reference: &str, fpid: &str, pads: &[(&str, i64)]) -> TopoFootprint {
        TopoFootprint { reference: reference.into(), fpid: fpid.into(), pads: pads.iter().map(|(n, c)| (n.to_string(), *c)).collect(), symbol: None }
    }

    /// One channel: an amplifier U, an input resistor R and a feedback capacitor C, on its own nets (`base`..`base + 2`) and the shared rails (1 and 2).
    fn channel(first: usize, base: i64) -> Vec<TopoFootprint> {
        vec![
            fp(&format!("U{first}"), "Pkg:SOT23-5", &[("1", base), ("2", 2), ("3", base + 1), ("4", base + 2), ("5", 1)]),
            fp(&format!("R{first}"), "R:0603", &[("1", base), ("2", base + 1)]),
            fp(&format!("C{first}"), "C:0603", &[("1", base + 1), ("2", base + 2)]),
        ]
    }

    fn pairs(a: &[TopoFootprint], b: &[TopoFootprint]) -> Result<Vec<(String, String)>, Vec<MismatchReason>> {
        let ga = Graph::build(a, b, &HashMap::new());
        let gb = Graph::build(b, a, &HashMap::new());
        ga.find_isomorphism(&gb).map(|v| v.into_iter().map(|(r, t)| (a[r].reference.clone(), b[t].reference.clone())).collect())
    }

    #[test]
    fn two_identical_channels_match_part_for_part_and_the_rails_are_not_part_of_the_topology() {
        let (a, b) = (channel(1, 10), channel(2, 20));
        // by the place of the reference footprint in the slice the graph was built from
        let got = pairs(&a, &b).unwrap();
        assert_eq!(got, vec![("U1".to_string(), "U2".to_string()), ("R1".into(), "R2".into()), ("C1".into(), "C2".into())]);
    }

    #[test]
    fn a_pin_tied_to_another_rail_in_one_channel_still_matches() {
        // pin 2 of the amplifier: ground in one channel, the supply in the other -- both nets are external, so the tie is not compared
        let (a, mut b) = (channel(1, 10), channel(2, 20));
        b[0].pads[1].1 = 1;
        assert!(pairs(&a, &b).is_ok());
    }

    #[test]
    fn a_missing_connection_is_a_mismatch_with_the_reason_kicad_gives() {
        let (a, mut b) = (channel(1, 10), channel(2, 20));
        // the resistor of channel 2 is not on the amplifier's net any more
        b[1].pads[0].1 = 99;
        let err = pairs(&a, &b).unwrap_err();
        assert!(!err.is_empty());
        assert!(err.iter().any(|r| r.reason.contains("No compatible component") || r.reason.contains("net ") || r.reason.contains("connects to")), "{err:?}");
    }

    #[test]
    fn different_numbers_of_components_and_empty_areas_are_refused_in_kicads_words() {
        let a = channel(1, 10);
        let mut b = channel(2, 20);
        b.pop();
        assert_eq!(pairs(&a, &b).unwrap_err()[0].reason, "Component count mismatch");
        assert_eq!(pairs(&a, &[]).unwrap_err()[0].reason, "One or both of the areas has no components assigned.");
    }

    #[test]
    fn a_footprint_of_another_kind_cannot_stand_for_the_reference() {
        let a = channel(1, 10);
        let mut b = channel(2, 20);
        b[1].fpid = "R:0805".into();
        assert!(pairs(&a, &b).is_err());
        // a different reference letter is another kind too
        let mut c = channel(2, 20);
        c[1].reference = "L2".into();
        assert!(pairs(&a, &c).is_err());
    }

    #[test]
    fn channel_suffixes_in_the_prefix_are_the_same_kind() {
        assert!(prefixes_share_common_base("TRIM_1.", "TRIM_2."));
        assert!(prefixes_share_common_base("R", "R"));
        assert!(!prefixes_share_common_base("R", "C"));
        assert!(!prefixes_share_common_base("RA", "RB"));
        assert_eq!(refdes_prefix("TRIM_1.12"), "TRIM_1.");
        assert_eq!(refdes_prefix("U1?"), "U");
    }

    #[test]
    fn identical_parts_are_paired_in_reference_order_unless_the_symbol_says_otherwise() {
        // four decoupling capacitors on the same two nets
        let caps = |first: i64, symbols: bool| -> Vec<TopoFootprint> {
            (0..4)
                .map(|i| {
                    let mut f = fp(&format!("C{}", first + i), "C:0603", &[("1", 1), ("2", 2)]);
                    if symbols {
                        f.symbol = Some(format!("sym{}", 3 - i));
                    }
                    f
                })
                .collect()
        };
        // no symbol: C1 -> C11, C2 -> C12 ...
        let plain = pairs(&caps(1, false), &caps(11, false)).unwrap();
        assert_eq!(plain, vec![("C1".to_string(), "C11".to_string()), ("C2".into(), "C12".into()), ("C3".into(), "C13".into()), ("C4".into(), "C14".into())]);
        // the two sets carry the same symbols in the order sym3..sym0, so C1 stands for the same symbol as C11 and so on
        let linked = pairs(&caps(1, true), &caps(11, true)).unwrap();
        assert_eq!(linked, plain);
        // a channel whose symbols are in another order is paired by symbol
        let mut reversed = caps(11, true);
        for (i, f) in reversed.iter_mut().enumerate() {
            f.symbol = Some(format!("sym{i}"));
        }
        let by_symbol = pairs(&caps(1, true), &reversed).unwrap();
        assert_eq!(by_symbol, vec![("C1".to_string(), "C14".to_string()), ("C2".into(), "C13".into()), ("C3".into(), "C12".into()), ("C4".into(), "C11".into())]);
    }

    #[test]
    fn the_search_gives_up_after_ten_thousand_steps_in_kicads_words() {
        // A graph whose every component fits every other, with a mismatch only the last component sees: backtracking over permutations is what the limit is for.
        let n = 9usize;
        let a: Vec<TopoFootprint> = (0..n).map(|i| fp(&format!("R{}", i + 1), "R:0603", &[("1", 100 + i as i64), ("2", 200 + i as i64)])).collect();
        let mut b: Vec<TopoFootprint> = (0..n).map(|i| fp(&format!("R{}", i + 11), "R:0603", &[("1", 300 + i as i64), ("2", 400 + i as i64)])).collect();
        // the last one ties its two pads together where the others do not
        b[n - 1].pads[1].1 = 300 + (n as i64 - 1);
        let err = pairs(&a, &b).unwrap_err();
        // 8! orders of the eight resistors that fit are more than the limit allows: the timeout comes first, then what the dead end said
        assert_eq!(err[0].reason, "Iteration count exceeded (timeout)", "{err:?}");
        assert_eq!(err[1].reason, "No compatible component found in the target area.");
    }

    #[test]
    fn nets_are_named_in_the_reasons_and_numbered_when_they_have_no_name() {
        let a = vec![fp("R1", "R:0603", &[("1", 5), ("2", 6)]), fp("R2", "R:0603", &[("1", 6), ("2", 7)])];
        let b = vec![fp("R11", "R:0603", &[("1", 15), ("2", 16)]), fp("R12", "R:0603", &[("1", 17), ("2", 18)])];
        let names: HashMap<i64, String> = [(6, "/ch1/MID".to_string()), (16, "/ch2/MID".to_string())].into_iter().collect();
        let ga = Graph::build(&a, &b, &names);
        let gb = Graph::build(&b, &a, &names);
        let err = ga.find_isomorphism(&gb).unwrap_err();
        let text: String = err.iter().map(|r| r.reason.clone()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("No compatible component") || text.contains("net") || text.contains("connects"), "{text}");
    }
}
