#!/usr/bin/env python3
"""Analyze predeclared process-pair means; never pool individual get samples."""
from pathlib import Path
import argparse, hashlib, json, math, random, statistics
T_ONE_95_DF29=1.6991270265334972
T_TWO_95_DF29=2.045229642132703

def percentile(a,q):return sorted(a)[max(0,math.ceil(len(a)*q)-1)]
def intervals(values,seed,resamples=100000,block_length=3):
 assert len(values)==30 and all(math.isfinite(x)for x in values)
 n=len(values);mean=statistics.mean(values);se=statistics.stdev(values)/math.sqrt(n)
 boot=random.Random(seed);block=random.Random(seed+1)
 independent=[];moving=[]
 for _ in range(resamples):
  independent.append(sum(values[boot.randrange(n)]for _ in range(n))/n)
  draw=[]
  for _ in range(math.ceil(n/block_length)):
   start=block.randrange(n);draw.extend(values[(start+j)%n]for j in range(block_length))
  moving.append(sum(draw[:n])/n)
 independent.sort();moving.sort()
 def bounds(samples,low,high):return [percentile(samples,low),percentile(samples,high)]
 ci90={'student_t':[mean-T_ONE_95_DF29*se,mean+T_ONE_95_DF29*se], 'pair_bootstrap':bounds(independent,.05,.95),'moving_block_bootstrap':bounds(moving,.05,.95)}
 ci95={'student_t':[mean-T_TWO_95_DF29*se,mean+T_TWO_95_DF29*se], 'pair_bootstrap':bounds(independent,.025,.975),'moving_block_bootstrap':bounds(moving,.025,.975)}
 envelope=[min(x[0]for x in ci90.values()),max(x[1]for x in ci90.values())]
 return {'process_pairs':n,'paired_mean_percent':mean,'paired_median_percent':statistics.median(values),'paired_stdev_percent':statistics.stdev(values),'values_percent':values,'ci_two_sided_90_percent':ci90,'ci_two_sided_95_percent':ci95,'conservative_one_sided_95_lower_percent':envelope[0],'conservative_one_sided_95_upper_percent':envelope[1],'conservative_two_sided_90_envelope_percent':envelope,'positive_pairs':sum(x>0 for x in values),'above_10_percent_pairs':sum(x>10 for x in values),'bootstrap_seed':seed,'bootstrap_resamples':resamples,'moving_block_length':block_length,'uncertainty_limit':'Approximate finite-sample inference from process pairs; host/time dependence and all outliers remain visible.'}
def analyze(receipt,protocol):
 if not (receipt.get('complete') and receipt.get('passed') and len(receipt.get('runs',[]))==120 and all(r.get('passed')for r in receipt['runs'])):
  return {'profile':'process-pair-original-full-journey-tail-analysis','complete':True,'valid_capture':False,'decision':'inconclusive_review_open','error':'Incomplete or failed fixed-schedule capture; no pairs excluded/replaced'}
 p99={};supplement={}
 for series in ('candidate','identical'):
  values=[];rows=[]
  for b in range(1,31):
   pair={r['arm']:r for r in receipt['runs']if r['series']==series and r['block']==b}
   assert set(pair)=={'baseline','candidate'} and all(r['passed']for r in pair.values())
   a=pair['baseline']['metrics']['batched_get'];c=pair['candidate']['metrics']['batched_get']
   value=100*(c['p99_ns']-a['p99_ns'])/a['p99_ns'];values.append(value)
   order=[r['arm']for r in receipt['runs']if r['series']==series and r['block']==b]
   rows.append({'block':b,'order':order,'baseline_process_p99_ns':a['p99_ns'],'candidate_process_p99_ns':c['p99_ns'],'p99_change_percent':value,'throughput_change_percent':100*(c['operations_per_second']/a['operations_per_second']-1),'p50_change_percent':100*(c['p50_ns']/a['p50_ns']-1),'p95_change_percent':100*(c['p95_ns']/a['p95_ns']-1)})
  p99[series]=intervals(values,protocol['bootstrap_seed'],protocol['bootstrap_resamples'],protocol['moving_block_length_pairs'])
  supplement[series]={'paired_rows':rows,'mean_baseline_process_p99_ns':statistics.mean(x['baseline_process_p99_ns']for x in rows),'mean_candidate_process_p99_ns':statistics.mean(x['candidate_process_p99_ns']for x in rows),'order_subgroups':{arm:{'pairs':len([x for x in rows if x['order'][0]==arm]),'mean_p99_change_percent':statistics.mean(x['p99_change_percent']for x in rows if x['order'][0]==arm)}for arm in ('baseline','candidate')},'mean_throughput_change_percent':statistics.mean(x['throughput_change_percent']for x in rows),'mean_p50_change_percent':statistics.mean(x['p50_change_percent']for x in rows),'mean_p95_change_percent':statistics.mean(x['p95_change_percent']for x in rows)}
 margin=protocol['target_mean_ratio_percent'];control=p99['identical']['conservative_two_sided_90_envelope_percent'];resolves=control[0]>-margin and control[1]<margin
 valid=receipt['complete']and receipt['passed']and receipt.get('source_unchanged')and len(receipt['runs'])==120 and receipt['filesystem']['type']=='tmpfs'
 if valid and resolves and p99['candidate']['conservative_one_sided_95_upper_percent']<margin:decision='close_exact_artifact_repeated_tail_review'
 elif valid and p99['candidate']['conservative_one_sided_95_lower_percent']>margin:decision='regression_beyond_margin_review_open'
 else:decision='inconclusive_review_open'
 return {'profile':'process-pair-original-full-journey-tail-analysis','complete':True,'valid_capture':bool(valid),'control_resolves_10_percent_margin':resolves,'decision':decision,'p99':p99,'supplementary':supplement,'claim_scope':'Only the process-pair arithmetic mean sampled-p99 ratio for these exact artifacts and original full tmpfs journey; no universal p99 equality, broad tail parity or causal code claim.','earlier_evidence':'Original three adverse full pairs and seven reduced-later-work focused pairs are retained alongside, never pooled into this predeclared inference.'}
def main():
 parser=argparse.ArgumentParser();parser.add_argument('receipt',type=Path);parser.add_argument('--output',type=Path,required=True);a=parser.parse_args();r=json.loads(a.receipt.read_text());w=Path(__file__).parent;protocol=json.loads((w/'PROTOCOL.json').read_text())
 def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
 try:
  assert r['protocol_sha256']==sha(w/'PROTOCOL.json') and r['analysis_sha256']==sha(__file__),'analysis/protocol drift'
  schedule_keys=list(protocol['schedule'][0])
  assert [{k:x[k]for k in schedule_keys}for x in r['runs']]==protocol['schedule'],'fixed schedule differs'
  for row in r['runs']:
   assert sha(row['stdout'])==row['stdout_sha256'] and sha(row['stderr'])==row['stderr_sha256'],'raw log drift'
   if row['passed']:
    raw=json.loads(Path(row['stdout']).read_text());lib=r['frozen_libraries'][row['series']][row['arm']]
    assert Path(raw['provider']).resolve()==Path(lib['path']).resolve() and sha(lib['path'])==lib['sha256'],'actual provider/artifact differs'
    samples=next(x['latency_ns']for x in raw['metrics']if x['phase']=='batched_get')
    assert len(samples)==protocol['configuration']['batched_reads'] and percentile(samples,.99)==row['metrics']['batched_get']['p99_ns'],'actual sampled p99 differs'
  assert sha(r['executable']['path'])==r['executable']['sha256'],'executable drift'
  out=analyze(r,protocol)
 except (AssertionError,KeyError,ValueError,OSError)as error:
  out={'profile':'process-pair-original-full-journey-tail-analysis','complete':True,'valid_capture':False,'decision':'inconclusive_review_open','error':repr(error)}
 out['receipt_sha256']=sha(a.receipt);a.output.write_text(json.dumps(out,indent=2)+'\n');print(json.dumps({'decision':out['decision'],'candidate_mean':out.get('p99',{}).get('candidate',{}).get('paired_mean_percent'),'candidate_upper':out.get('p99',{}).get('candidate',{}).get('conservative_one_sided_95_upper_percent'),'control_envelope':out.get('p99',{}).get('identical',{}).get('conservative_two_sided_90_envelope_percent'),'error':out.get('error')},indent=2))
if __name__=='__main__':main()
