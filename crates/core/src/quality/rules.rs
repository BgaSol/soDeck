use super::{probe::Outcome,Policy};
use serde_json::{json,Value};
#[derive(Default)]
pub(super) struct Rules {pub slow:bool,pub unresponsive:bool,slow_run:u8,fast_run:u8,timeout_run:u8,success_run:u8}
impl Rules {
    pub fn observe(&mut self,sample:&Outcome,policy:&Policy) {
        if let Some(ms)=sample.duration_ms.filter(|_|sample.outcome=="succeeded") {
            self.timeout_run=0;self.success_run=self.success_run.saturating_add(1);
            self.slow_run=if ms>policy.slow_ms as f64{self.slow_run.saturating_add(1)}else{0};
            self.fast_run=if ms<=policy.recovery_ms as f64{self.fast_run.saturating_add(1)}else{0};
            if self.slow_run>=3{self.slow=true;}if self.fast_run>=3{self.slow=false;}
            if self.success_run>=3{self.unresponsive=false;}
        }else{
            self.slow_run=0;self.fast_run=0;self.success_run=0;
            self.timeout_run=if sample.outcome=="CHECK_TIMEOUT"&&sample.sent{self.timeout_run.saturating_add(1)}else{0};
            if self.timeout_run>=3{self.unresponsive=true;}
        }
    }
}
pub(super) fn statistics(samples:&[&Outcome])->Value {
    let mut successes=Vec::new();let mut timeouts=0;let mut skipped=0;let mut sent=0;let mut pairs=0;let mut difference=0.0;let mut previous:Option<(f64,u64)>=None;
    for sample in samples {
        sent+=u32::from(sample.sent);
        if let Some(ms)=sample.duration_ms.filter(|_|sample.outcome=="succeeded") {
            successes.push(ms);
            if let Some((last,time))=previous {if sample.observed_at.saturating_sub(time)<=6000{pairs+=1;difference+=(ms-last).abs();}}
            previous=Some((ms,sample.observed_at));
        }else{previous=None;if sample.sent&&sample.outcome=="CHECK_TIMEOUT"{timeouts+=1;}else{skipped+=1;}}
    }
    successes.sort_by(f64::total_cmp);let n=successes.len();let complete=n+timeouts;
    json!({"successes":n,"sent":sent,"timeouts":timeouts,"skipped":skipped,"pairs":pairs,
        "medianMs":(n>=10).then(||if n%2==0{(successes[n/2-1]+successes[n/2])/2.0}else{successes[n/2]}),
        "p95Ms":(n>=10).then(||successes[(n*95).div_ceil(100)-1]),
        "variationMs":(pairs>=10).then(||difference/pairs as f64),
        "timeoutRate":(complete>0).then(||timeouts as f64/complete as f64)})
}

#[cfg(test)]mod tests {
    use super::*;
    fn sample(code:&'static str,ms:Option<f64>,time:u64)->Outcome {Outcome{observed_at:time,outcome:code,sent:code=="succeeded"||code=="CHECK_TIMEOUT",duration_ms:ms,queue_ms:0,open_ms:0}}
    #[test]fn thresholds_have_three_sample_hysteresis_and_independent_recovery(){
        let mut rules=Rules::default();let policy=Policy::default();
        for _ in 0..2{rules.observe(&sample("succeeded",Some(501.),0),&policy);}assert!(!rules.slow);
        rules.observe(&sample("succeeded",Some(501.),0),&policy);assert!(rules.slow);
        for _ in 0..5{rules.observe(&sample("succeeded",Some(400.),0),&policy);}assert!(rules.slow);
        for _ in 0..3{rules.observe(&sample("CHECK_TIMEOUT",None,0),&policy);}assert!(rules.unresponsive);
        for _ in 0..3{rules.observe(&sample("succeeded",Some(400.),0),&policy);}assert!(!rules.unresponsive);assert!(rules.slow);
        for _ in 0..3{rules.observe(&sample("succeeded",Some(350.),0),&policy);}assert!(!rules.slow);
        for _ in 0..10{rules.observe(&sample("SCHEDULING_DELAY",None,0),&policy);}assert!(!rules.unresponsive);
    }
    #[test]fn statistics_exclude_cancellation_and_break_pairs_at_gaps(){
        let mut samples:Vec<_>=(0..9).map(|n|sample("succeeded",Some(n as f64),n*5000)).collect();
        assert!(statistics(&samples.iter().collect::<Vec<_>>())["p95Ms"].is_null());
        samples.push(sample("succeeded",Some(9.),45000));samples.push(sample("CHECK_TIMEOUT",None,50000));samples.push(sample("CANCELED",None,55000));samples.push(sample("succeeded",Some(10.),60000));
        let stats=statistics(&samples.iter().collect::<Vec<_>>());assert_eq!(stats["successes"],11);assert_eq!(stats["pairs"],9);assert!(stats["variationMs"].is_null());assert_eq!(stats["timeoutRate"],1./12.);assert_eq!(stats["p95Ms"],10.);
        samples.push(sample("succeeded",Some(11.),65000));assert_eq!(statistics(&samples.iter().collect::<Vec<_>>())["variationMs"],1.);
    }
}
