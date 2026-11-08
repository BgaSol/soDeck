import {mountTerminal} from './terminal/viewport.js';
import {createSessionAdapter} from './terminal/session.js';
import {createLocation} from './location.js';
const session=createSessionAdapter();
mountTerminal({session,createLocation:terminal=>{
  const feature=createLocation(terminal);
  return {...feature,mount(value){const descriptor=session.current().descriptor;feature.mount({...value,data:{...value.data,streamId:descriptor.ptyId,title:descriptor.title,openedContext:descriptor.context,integration:descriptor.integration}});}};
}});
