//! KiCad's SPICE simulator, the part of it that is not a window (`eeschema/sim/`, package P21; docs/parity/PARITY-sch.md).
//!
//! The architecture is the user's (docs/HANDOFF.md, "Next tasks", item 3):
//! - the SPICE netlist is kicad-cli's (`kicad-cli sch export netlist --format spice` on the derived schematic), made into the netlist KiCad's
//!   simulator hands ngspice (`SPICE_CIRCUIT_MODEL::GetNetlist`: the analysis of the tab, `.save all`, `.probe alli`, ...) by [`netlist`];
//! - ngspice runs as a separate program, `ngspice -b` in batch mode ([`ngspice`]), on a deck that sources that netlist and does what KiCad
//!   asks of libngspice around a run ([`deck`]: the compatibility mode, tuners, user-defined signals, measurements, Fourier analyses), and
//!   writes raw files that [`raw`] reads back;
//! - what the simulator knows of the circuit (its nets, items, pins and currents) is read from kicad-cli's SPICE and XML netlists
//!   ([`netlist::circuit`], [`xml`]);
//! - the workbook (`.wbk`) is read and written as KiCad does ([`workbook`]).
//!
//! What this rules out: libngspice's background run (`NGSPICE::Run` is `bg_run`) and its live updates (`SIMULATOR_FRAME_UI::OnSimRefresh` on a
//! 50 ms timer while the simulation runs). A batch run is over when its process ends, so the plot shows the result when it is there, and Stop
//! Simulation kills the process (`NGSPICE::Stop` is `bg_halt`, which keeps the vectors made so far).

pub mod command;
pub mod deck;
pub mod library;
pub mod netlist;
pub mod ngspice;
pub mod raw;
pub mod value;
pub mod workbook;
pub mod xml;
