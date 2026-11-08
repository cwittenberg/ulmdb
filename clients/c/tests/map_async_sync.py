#!/usr/bin/env python3
"""Focused real-syscall proof for the C unit worker; no timing/power-loss claim."""
import argparse,hashlib,itertools,json,os,pathlib,re,signal,subprocess,sys
p=argparse.ArgumentParser();p.add_argument('--executable',required=True,type=pathlib.Path);p.add_argument('--output',required=True,type=pathlib.Path);a=p.parse_args()
exe=a.executable.resolve();digest=hashlib.sha256(exe.read_bytes()).hexdigest();a.output.mkdir(parents=True,exist_ok=False);rows=[]
def save():
 (a.output/'receipt.json').write_text(json.dumps({'status':'pass' if len(rows)==12 and all(x.get('passed') for x in rows) else 'incomplete-or-failed','executable':str(exe),'sha256':digest,'cases':rows,'scope':'actual public-C worker syscall/lifecycle checks, not power-loss qualification'},indent=2)+'\n')
try:
 for fmt,external,skip in itertools.product([2,3,4],[False,True],[False,True]):
  name=f'format{fmt}-external{int(external)}-skip{int(skip)}';d=a.output/name;d.mkdir();trace=d/'trace.log';env=os.environ.copy();env.update(ULMDB_ASYNC_SYNC_FORMAT=str(fmt),ULMDB_ASYNC_SYNC_NOLOCK=str(int(external)),ULMDB_ASYNC_SYNC_SKIP=str(int(skip)))
  cmd=['strace','-f','-s','512','-e','trace=msync,fsync,fdatasync,write','-o',str(trace),str(exe),'--exact','write_map::tests::public_nonforced_async_sync_worker','--nocapture']
  row={'name':name,'command':cmd};rows.append(row);proc=None
  try:
   proc=subprocess.Popen(cmd,env=env,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
   try:out,err=proc.communicate(timeout=15)
   except BaseException:
    try: os.killpg(proc.pid,signal.SIGKILL)
    except ProcessLookupError: pass
    out,err=proc.communicate();(d/'stdout').write_bytes(out);(d/'stderr').write_bytes(err);row['exit_code']=proc.returncode
    raise
   (d/'stdout').write_bytes(out);(d/'stderr').write_bytes(err);row['exit_code']=proc.returncode
   assert proc.returncode==0,(name,err.decode(errors='replace'))
   text=trace.read_text();start=text.index('MAPSYNC_ASYNC_BEGIN');end=text.index('MAPSYNC_ASYNC_END',start);part=text[start:end];force=text[text.index('MAPSYNC_FORCE_BEGIN'):text.index('MAPSYNC_FORCE_END')]
   scheduled=[line for line in part.splitlines() if 'msync(' in line];assert bool(scheduled)!=skip,(name,scheduled)
   assert all('MS_ASYNC' in line and re.search(r'= 0\s*$',line) for line in scheduled),(name,scheduled)
   assert 'fsync(' not in part and 'fdatasync(' not in part,(name,part)
   private=re.search(rb'MAPSYNC_PRIVATE=([0-9a-f]+):(\d+)',err);assert private
   base=int(private[1],16);limit=base+int(private[2])
   for line in scheduled:
    address=int(re.search(r'msync\((0x[0-9a-f]+)',line)[1],16);assert not base<=address<limit,(name,'private writer was scheduled')
   fences=[line for line in force.splitlines() if re.search(r'\b(?:fsync|fdatasync)\(',line)];assert fences,(name,'force had no actual fence');assert all('MS_ASYNC' not in line for line in force.splitlines() if 'msync(' in line)
   row.update(passed=True,async_msyncs=len(scheduled),force_fences=len(fences),artifacts={n:hashlib.sha256((d/n).read_bytes()).hexdigest() for n in ['stdout','stderr','trace.log']})
  except BaseException as error:
   row['failure']=repr(error);save();raise
  save()
 assert hashlib.sha256(exe.read_bytes()).hexdigest()==digest
 print('PASS: 12 mapped async/NOSYNC/forced syscall journeys; source selection and debt bytes unchanged')
except BaseException:
 save();raise
