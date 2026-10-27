import type {Channel,Connect} from './channel';
import {http} from './http';
import {socks} from './socks';
export async function proxy(client:Channel,connect:Connect):Promise<boolean> {
  const deadline=Date.now()+10000;
  const first=await client.readable(deadline);if(!first.length)return false;
  return first[0]===5?socks(client,connect,deadline):http(client,connect,deadline);
}
