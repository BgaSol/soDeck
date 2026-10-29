export type SecretInput={password:string;privateKeyPath:string;passphrase:string};
export class Secrets {
  private values=new Map<string,SecretInput>();
  get(id:string):SecretInput{return this.values.get(id)??{password:'',privateKeyPath:'',passphrase:''};}
  set(id:string,patch:Partial<SecretInput>){
    const value={...this.get(id),...patch};
    const size=(v:SecretInput)=>new TextEncoder().encode(v.password+v.privateKeyPath+v.passphrase).length;
    let bytes=size(value);for(const [key,v] of this.values)if(key!==id)bytes+=size(v);
    if(bytes>256*1024)throw Error('凭据输入已达 256 KiB 上限，请先完成其他配置');
    this.values.set(id,value);
  }
  clear(id:string){this.values.delete(id);}
  clearAll(){this.values.clear();}
  has(id:string){return Object.values(this.get(id)).some(Boolean);}
}
