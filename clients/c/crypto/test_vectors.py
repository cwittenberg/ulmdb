#!/usr/bin/env python3
"""Focused callback ABI checks; uses OpenSSL upstream known-answer facts."""
import ctypes as C,json,pathlib,sys,concurrent.futures
class Value(C.Structure):_fields_=[('size',C.c_size_t),('data',C.c_void_p)]
class Hooks(C.Structure):_fields_=[('str2key',C.c_void_p),('encrypt',C.c_void_p),('checksum',C.c_void_p),('keysize',C.c_int),('tagsize',C.c_int),('sumsize',C.c_int)]
Crypt=C.CFUNCTYPE(C.c_int,C.POINTER(Value),C.POINTER(Value),C.POINTER(Value),C.c_int)
Parse=C.CFUNCTYPE(C.c_int,C.c_char_p,C.POINTER(Value))
def buffer(data):return C.create_string_buffer(data,len(data)) if data else None
def value(buf,n):return Value(n,C.addressof(buf)if buf is not None else None)
def call(fn,src,key,iv,tag,enc,inplace=False):
 source=buffer(src);out=source if inplace else buffer(b'\xa5'*len(src));k=buffer(key);nonce=buffer(iv);mac=buffer(tag);s=value(source,len(src));d=value(out,len(src));keys=(Value*3)(value(k,len(key)),value(nonce,len(iv)),value(mac,len(tag)));before=(s.size,s.data,d.size,d.data,[(x.size,x.data)for x in keys]);rc=fn(C.byref(s),C.byref(d),keys,enc);assert before==(s.size,s.data,d.size,d.data,[(x.size,x.data)for x in keys]);return rc,bytes(out)if out is not None else b'',bytes(mac)if mac is not None else b''
def main():
 library=C.CDLL(str(pathlib.Path(sys.argv[1]).resolve()));library.MDB_crypto.restype=C.POINTER(Hooks);hooks=library.MDB_crypto().contents;assert(hooks.keysize,hooks.tagsize,hooks.sumsize,hooks.checksum)==(32,16,0,None);crypt=Crypt(hooks.encrypt);parse=Parse(hooks.str2key);vectors=json.loads(pathlib.Path(__file__).with_name('vectors.json').read_text())['vectors']
 for v in vectors:
  key,iv,plain,cipher,tag=[bytes.fromhex(v[k])for k in ['key','iv','plaintext','ciphertext','tag']];rc,actual,mac=call(crypt,plain,key,iv,bytes(16),1);assert(rc,actual,mac)==(0,cipher,tag);assert call(crypt,cipher,key,iv,tag,0)[:2]==(0,plain);assert call(crypt,plain,key,iv,bytes(16),1,True)[1:]==(cipher,tag)
  bad=bytes([tag[0]^1])+tag[1:];rc,actual,_=call(crypt,cipher,key,iv,bad,0);assert rc!=0 and actual==bytes(len(plain))
 key=bytes(range(32));plain=bytes(range(37))
 for n in range(1,129):
  iv=bytes(i%256 for i in range(n));rc,cipher,tag=call(crypt,plain,key,iv,bytes(16),1);assert rc==0;assert call(crypt,cipher,key,iv,tag,0)[:2]==(0,plain)
 iv=bytes(16);_,cipher,tag=call(crypt,plain,key,iv,bytes(16),1)
 for changed_key,changed_iv,changed_cipher,changed_tag in [(bytes([1])+key[1:],iv,cipher,tag),(key,bytes([1])+iv[1:],cipher,tag),(key,iv,bytes([cipher[0]^1])+cipher[1:],tag),(key,iv,cipher,tag[:-1]+bytes([tag[-1]^1]))]:
  rc,data,_=call(crypt,changed_cipher,changed_key,changed_iv,changed_tag,0);assert rc!=0 and data==bytes(len(plain))
 for k,i,t,e in [(key[:-1],iv,tag,1),(key+b'x',iv,tag,1),(key,b'',tag,1),(key,bytes(129),tag,1),(key,iv,tag[:-1],1),(key,iv,tag+b'x',1),(key,iv,tag,2)]:
  rc,data,_=call(crypt,plain,k,i,t,e);assert rc!=0 and data==b'\xa5'*len(plain)
 out=buffer(bytes(32));v=value(out,32);original=(v.size,v.data)
 for text in [key.hex().encode(),key.hex().upper().encode()]:assert parse(text,C.byref(v))==0 and bytes(out)==key and(v.size,v.data)==original
 for text in [None,b'',b'passphrase',b'0'*63,b'0'*65,b'g'*64,b' '+b'0'*63,b'0x'+b'0'*64]:
  C.memset(out,0x5a,32);assert parse(text,C.byref(v))!=0 and bytes(out)==b'Z'*32 and(v.size,v.data)==original
 src=buffer(plain);dst=buffer(b'\x5a'*len(plain));s=value(src,len(plain));d=value(dst,len(plain)-1);# Keep every callback buffer owned through the call.
 kb,ib,tb=buffer(key),buffer(iv),buffer(tag);keys=(Value*3)(value(kb,32),value(ib,16),value(tb,16));assert crypt(C.byref(s),C.byref(d),keys,1)!=0 and bytes(dst)==b'Z'*len(plain);assert crypt(None,C.byref(d),keys,1)!=0;assert crypt(C.byref(s),None,keys,1)!=0;assert crypt(C.byref(s),C.byref(d),None,1)!=0
 overlap=buffer(bytes(64));s=Value(32,C.addressof(overlap));d=Value(32,C.addressof(overlap)+1);assert crypt(C.byref(s),C.byref(d),keys,1)!=0 and bytes(overlap)==bytes(64)
 def worker(i):
  k=bytes([i])*32
  for j in range(16):
   nonce=i.to_bytes(8,'little')+j.to_bytes(8,'little');rc,cipher,tag=call(crypt,plain,k,nonce,bytes(16),1);assert rc==0;assert call(crypt,cipher,k,nonce,tag,0)[:2]==(0,plain)
 with concurrent.futures.ThreadPoolExecutor(max_workers=4)as pool:list(pool.map(worker,range(1,5)))
 print(f'crypto-callback-ok: {len(vectors)} knownanswers, IV1..128, tamperedkey/IV/cipher/tag zerooutput, stricthex/shape/alias, inplace, parallelpercallcontexts')
if __name__=='__main__':main()
