#!/usr/bin/env python3
"""Execute the reviewed fixed schedule only after Root coordinates its window."""
from pathlib import Path
from types import SimpleNamespace
from datetime import datetime,timezone
import argparse,hashlib,importlib.util,json,os,platform,shutil,signal,statistics,subprocess,threading,time
W=Path(__file__).resolve().parent;ROOT=W.parents[2]
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def write(p,v):p.write_text(json.dumps(v,indent=2)+'\n')
def native_sources():
 files=[ROOT/'Cargo.toml',ROOT/'Cargo.lock']
 for area in ['crates','clients/c/src']:
  files.extend(p for p in (ROOT/area).rglob('*')if p.is_file() and p.suffix in ['.rs','.toml'])
 return [{'path':str(p.relative_to(ROOT)),'sha256':sha(p)}for p in sorted(files)]
def main():
 parser=argparse.ArgumentParser();parser.add_argument('--execute',action='store_true');parser.add_argument('--window-label',required=True);a=parser.parse_args()
 if not a.execute:parser.error('No workload started: explicit --execute and coordinated Root window required')
 O=W/'capture';O.mkdir(exist_ok=False);P=O/'receipt.json';plan=json.loads((W/'PROTOCOL.json').read_text());inputs=plan['inputs'];R={'profile':plan['profile'],'complete':False,'passed':False,'window_label':a.window_label,'started_utc':datetime.now(timezone.utc).isoformat(),'protocol_sha256':sha(W/'PROTOCOL.json'),'protocol_text_sha256':sha(W/'PROTOCOL.md'),'analysis_sha256':sha(W/'analyze_confirmation.py'),'runner_sha256':sha(__file__),'runs':[],'host_samples':[],'host':{'platform':platform.platform(),'cpu_count':os.cpu_count(),'affinity':sorted(os.sched_getaffinity(0)),'isolation_claim':False,'team_quiet_window':'Root coordinated; external jobs recorded, not killed'},'configuration':plan['configuration']}
 stop=threading.Event();active=[]
 class OwnedProcess:
  def __init__(self,*args,**kwargs):kwargs['start_new_session']=True;self.p=subprocess.Popen(*args,**kwargs);active.append(self.p)
  def __getattr__(self,key):return getattr(self.p,key)
  def kill(self):
   try:os.killpg(self.p.pid,signal.SIGKILL)
   except ProcessLookupError:pass
 def observe():
  while not stop.is_set():
   row={'monotonic':time.monotonic(),'utc':datetime.now(timezone.utc).isoformat(),'loadavg':os.getloadavg()}
   for key,path in [('cpu_pressure','/proc/pressure/cpu'),('memory_pressure','/proc/pressure/memory'),('cpu_stat','/proc/stat')]:
    try:row[key]=Path(path).read_text()
    except OSError as e:row[key]=str(e)
   try:row['processes']=subprocess.run(['ps','-eo','pid,ppid,pcpu,pmem,stat,comm','--sort=-pcpu'],capture_output=True,text=True,check=True).stdout.splitlines()[:31]
   except Exception as e:row['processes']=str(e)
   R['host_samples'].append(row);stop.wait(1)
 watcher=threading.Thread(target=observe,daemon=True)
 try:
  for x in inputs.values():assert sha(x['path'])==x['sha256'],x
  for key in ['ULMDB_FAIL_AT','ULMDB_CRASH_AT','ULMDB_PAUSE_AT','ULMDB_TRACE_FILE']:assert key not in os.environ,('unexpected injector environment',key)
  staged={key:W/'inputs'/f"{key}-{Path(inputs[key]['path']).name}"for key in ['executable','baseline','candidate','source','monitor']}
  for key,path in staged.items():assert sha(path)==inputs[key]['sha256']
  libs={}
  for series in ['candidate','identical']:
   libs[series]={}
   for arm in ['baseline','candidate']:
    artifact='candidate'if series=='candidate'and arm=='candidate'else'baseline';dest=O/'libraries'/series/arm/'liblmdb.so.0';dest.parent.mkdir(parents=True);shutil.copy2(staged[artifact],dest);assert sha(dest)==inputs[artifact]['sha256'];libs[series][arm]=dest
  exe=O/'journey';shutil.copy2(staged['executable'],exe);assert sha(exe)==inputs['executable']['sha256']
  spec=importlib.util.spec_from_file_location('original_performance_monitor',staged['monitor']);h=importlib.util.module_from_spec(spec);spec.loader.exec_module(h)
  # Preserve original successful-run 1ms monitor/validation logic; wrap Popen
  # only to own/kill the benchmark's process group on timeout/cancellation.
  h.subprocess=SimpleNamespace(Popen=OwnedProcess,run=subprocess.run)
  ram=Path('/dev/shm');assert h.filesystem(ram)=='tmpfs';assert shutil.disk_usage(ram).free>512*1024*1024
  data=ram/f'ulmdb-f03-full-tail-confirmation-{os.getpid()}-{time.time_ns()}';data.mkdir()
  R['filesystem']={'root':str(data),'type':h.filesystem(data),'free_bytes_before':shutil.disk_usage(data).free};assert R['filesystem']['type']=='tmpfs'
  harness_before={str(x):sha(x)for x in [Path(__file__),W/'analyze_confirmation.py',W/'PROTOCOL.json',W/'PROTOCOL.md']};R['harness_before']=harness_before
  before=native_sources();R['root_sources_before']=before;R['frozen_libraries']={s:{arm:{'path':str(path),'sha256':sha(path)}for arm,path in arms.items()}for s,arms in libs.items()};R['executable']={'path':str(exe),'sha256':sha(exe)};write(P,R)
  args=SimpleNamespace(**plan['configuration']);watcher.start()
  for row in plan['schedule']:
   result=h.run_one(args,exe,libs[row['series']][row['arm']],data/row['database_name'],O/'runs'/row['database_name']);result.update(row)
   result['stdout_sha256']=sha(result['stdout']);result['stderr_sha256']=sha(result['stderr'])
   if result['passed']:
    expected_samples={'create_open':1,'initial_load':1,'batched_get':args.batched_reads,'single_get_transaction':args.single_reads,'update_batch1':args.write_transactions,'update_batch16':args.write_transactions,'close_reopen':args.reopens}
    if any(result['metrics'][phase]['sample_count']!=count for phase,count in expected_samples.items()):result.update(passed=False,error='full-workload sample count differs')
   R['runs'].append(result);write(P,R);print(f"{row['ordinal']:03}/120 {'PASS'if result['passed']else'FAIL'} {row['database_name']} {result['process_seconds']:.3f}s",flush=True)
  assert len(R['runs'])==120
  R['source_unchanged']=native_sources()==before
  assert R['source_unchanged'],'Root product source changed'
  assert all(sha(path)==value for path,value in harness_before.items()),'confirmation harness changed'
  for x in inputs.values():assert sha(x['path'])==x['sha256'],'original input drift'
  for key,path in staged.items():assert sha(path)==inputs[key]['sha256'],'staged input drift'
  assert sha(exe)==inputs['executable']['sha256']
  for series,arms in libs.items():
   for arm,path in arms.items():assert sha(path)==inputs['candidate'if series=='candidate'and arm=='candidate'else'baseline']['sha256']
  assert sorted(os.sched_getaffinity(0))==R['host']['affinity'],'affinity changed'
  images={json.dumps(row['checksums'],sort_keys=True)for row in R['runs']if row['passed']};assert len(images)==1,'logical image differs'
  R.update(complete=True,passed=all(row['passed']for row in R['runs']))
 except BaseException as error:
  R['error']=repr(error)
  for p in active:
   if p.poll()is None:
    try:os.killpg(p.pid,signal.SIGTERM)
    except ProcessLookupError:pass
    try:p.wait(timeout=2)
    except subprocess.TimeoutExpired:
     try:os.killpg(p.pid,signal.SIGKILL)
     except ProcessLookupError:pass
     p.wait()
 finally:
  stop.set()
  if watcher.is_alive():watcher.join(timeout=3)
  R['finished_utc']=datetime.now(timezone.utc).isoformat();write(P,R)
 print(json.dumps({'complete':R['complete'],'passed':R['passed'],'runs':len(R['runs']),'receipt':str(P)},indent=2),flush=True)
 return 0 if R['complete']and R['passed']else 1
if __name__=='__main__':raise SystemExit(main())
