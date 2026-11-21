// A fixed-height navigation skeleton has exactly one measured inline region.
export class TreeLayout {
  constructor(rowHeight=30){this.rowHeight=rowHeight;this.total=0;this.inlineIndex=-1;this.height=0;this.key='';}
  reset(total,anchor){const key=anchor?.node?.key||'';if(key!==this.key)this.height=0;this.key=key;this.total=total;this.inlineIndex=anchor?.index??-1;}
  measure(height){const next=this.inlineIndex<0?0:Math.max(0,height);if(Math.abs(next-this.height)<.5)return false;this.height=next;return true;}
  offset(index){return index*this.rowHeight+(this.inlineIndex>=0&&index>this.inlineIndex?this.height:0);}
  index(top){if(this.inlineIndex>=0){const begin=(this.inlineIndex+1)*this.rowHeight;if(top>=begin&&top<begin+this.height)return this.inlineIndex;if(top>=begin+this.height)top-=this.height;}return Math.max(0,Math.min(this.total-1,Math.floor(top/this.rowHeight)));}
  get size(){return this.total*this.rowHeight+(this.inlineIndex>=0?this.height:0);}
}
