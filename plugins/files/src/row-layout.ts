/** Prefix heights shared by windowing, keyboard movement and scroll anchors. */
export class RowLayout {
  readonly offsets:number[]=[0];
  constructor(readonly rows:readonly {mount:boolean}[]){for(let i=0;i<rows.length;i++)this.offsets.push(i+1);}
  get total(){return this.offsets.at(-1)!;}
  at(unit:number){
    let low=0,high=this.rows.length;
    while(low<high){const mid=(low+high)>>>1;if(this.offsets[mid+1]<=unit)low=mid+1;else high=mid;}
    return Math.min(low,Math.max(0,this.rows.length-1));
  }
  range(start:number,count:number,overscan=0){const first=this.at(Math.max(0,start-overscan)),end=Math.min(this.rows.length,this.at(start+count+overscan)+1);return {first,end};}
}
