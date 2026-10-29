import type {CatalogItem} from '@sodeck/sdk';
import type {Server} from './types';
export const catalogServer=(item:CatalogItem):Server=>({...item.server,accessPath:item.accessPath,groupId:item.groupId,membershipRevision:item.membershipRevision});
