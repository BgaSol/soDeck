/** A group is atomic; different settings groups can finish independently. */
export interface SettingsSaveParticipant {
  id:string;
  dirty:boolean;
  unresolvedOperations:string[];
  validate():Promise<void>;
  commit():Promise<void>;
  reconcile():Promise<boolean>;
  discard():Promise<void>;
}
export async function saveParticipants(participants:SettingsSaveParticipant[],saved:()=>void){
  if(participants.some(p=>p.unresolvedOperations.length))throw Error('请先核对尚未确定的操作结果');
  const batch=participants.filter(p=>p.dirty);
  for(const p of batch)await p.validate();
  for(const p of batch){await p.commit();saved();}
}
