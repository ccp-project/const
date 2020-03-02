extern crate portus;
extern crate slog;

use portus::ipc::Ipc;
use portus::lang::Scope;
use portus::{CongAlg, Datapath, DatapathInfo, DatapathTrait, Report};
use slog::{debug, warn};
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub enum Constant {
    Cwnd(u32),
    Rate { rate: u32, cwnd_cap: u32 },
}
pub struct CcpConstAlg {
    pub logger: Option<slog::Logger>,
    pub const_param: Constant,
}

pub struct CcpConstFlow<T: Ipc> {
    logger: Option<slog::Logger>,
    sc: Scope,
    control_channel : Datapath<T>,
    const_param : Constant,
}

impl<I: Ipc> CongAlg<I> for CcpConstAlg {
    type Flow = CcpConstFlow<I>;

    fn name() -> &'static str {
        "constant"
    }

    fn datapath_programs(&self) -> HashMap<&'static str, String> {
        let mut h = HashMap::default();
        h.insert(
            "constant",
            "
            (def (Report
                (volatile rtt 0)
                (volatile rin 0)
                (volatile rout 0)
                (volatile loss 0)
            ))
            (when true
                (:= Report.rtt Flow.rtt_sample_us)
                (:= Report.rin Flow.rate_outgoing)
                (:= Report.rout Flow.rate_incoming)
                (:= Report.loss (+ Report.loss Ack.lost_pkts_sample))
                (fallthrough)
            )
            (when (> Micros Flow.rtt_sample_us)
                (report)
                (:= Micros 0)
            )"
            .to_owned(),
        );

        h
    }

    fn new_flow(&self, mut control: Datapath<I>, _info: DatapathInfo) -> Self::Flow {
        let params = match self.const_param {
            Constant::Cwnd(c) => vec![("Cwnd", c)],
            Constant::Rate {
                rate: r,
                cwnd_cap: c,
            } => vec![("Cwnd", c), ("Rate", r)],
        };
        let sc = control.set_program("constant", Some(&params)).unwrap();
        CcpConstFlow {
            logger: self.logger.clone(),
            sc,
            control_channel: control,
            const_param: self.const_param,
        }
    }
}

impl<T: Ipc> portus::Flow for CcpConstFlow<T> {
    fn on_report(&mut self, _sock_id: u32, m: Report) {
        let rtt = m
            .get_field("Report.rtt", &self.sc)
            .expect("expected rtt in report") as u32;
        let rin = m
            .get_field("Report.rin", &self.sc)
            .expect("expected rin in report") as u32;
        let rout = m
            .get_field("Report.rout", &self.sc)
            .expect("expected rout in report") as u32;
        let loss = m
            .get_field("Report.loss", &self.sc)
            .expect("expected loss in report") as u32;

        self.logger.as_ref().map(|log| {
            debug!(log, "report";
                "rtt(us)" => rtt,
                "rin(Bps)" => rin,
                "rout(Bps)" => rout,
                "loss(pkts)" => loss,
            );
        });

        let update = match self.const_param {
            Constant::Cwnd(c) => vec![("Cwnd", c)],
            Constant::Rate {
                rate: r,
                cwnd_cap: c,
            } => vec![("Cwnd", c), ("Rate", r)],
        };
        if let Err(e) = self.control_channel.update_field(&self.sc, &update) {
            self.logger.as_ref().map(|log| {
                warn!(log, "rate update error"; "err" => ?e,);
            });
        }
    }
}
