import {describe,it,expect} from 'vitest';
import {validate,type Shape} from './schema';
describe('cached schema validation',()=>{
  it('preserves Unicode lengths, uint64 and strict object constraints',()=>{
    const shape:Shape={type:'object',additionalProperties:false,required:['text','count'],properties:{text:{type:'string',minLength:1,maxLength:2},count:{type:'string',format:'uint64'}}},schema={$defs:{shape}};
    for(let n=0;n<2;n++){
      expect(validate(schema,shape,{text:'😀😀',count:'18446744073709551615'},'value')).toBeUndefined();
      for(const value of [{text:'😀😀a',count:'0'},{text:'a',count:'18446744073709551616'},{text:'a',count:'01'},{text:'a',count:'-1'},{text:'a',count:'0',unknown:true},{text:'a'}])expect(validate(schema,shape,value,'value')).toBeDefined();
    }
  });
  it('keeps references, branches and number safety checks',()=>{
    const schema={$defs:{number:{type:'integer',minimum:0}as Shape}},shape:Shape={anyOf:[{$ref:'#/$defs/number'},{type:'null'}]};
    expect(validate(schema,shape,null,'v')).toBeUndefined();expect(validate(schema,shape,12,'v')).toBeUndefined();for(const v of [-1,1.5,Infinity,Number.MAX_SAFE_INTEGER+1,'12'])expect(validate(schema,shape,v,'v')).toBeDefined();
  });
});
