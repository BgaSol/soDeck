export function path(points, maximum=100, gapMs=30000) {
  if (!points.length) return '';
  const end=points.at(-1).at,start=Math.min(end-300000,points[0].at),span=Math.max(1,end-start);
  let result='',open=false,previous;
  for(const p of points){if(p.value==null||!Number.isFinite(p.value)){open=false;previous=p.at;continue;}if(previous!=null&&p.at-previous>gapMs)open=false;const x=(p.at-start)/span*240,y=46-Math.max(0,Math.min(maximum,p.value))/maximum*44;result+=`${open?'L':'M'}${x.toFixed(1)},${y.toFixed(1)} `;open=true;previous=p.at;}
  return result.trim();
}
export function draw(svg,points,maximum=100,gapMs=30000){
  const ns='http://www.w3.org/2000/svg';svg.replaceChildren();
  const baseline=document.createElementNS(ns,'path');baseline.classList.add('baseline');baseline.setAttribute('d','M0 46H240');svg.append(baseline);
  const line=document.createElementNS(ns,'path');line.setAttribute('d',path(points,Math.max(1,maximum),gapMs));svg.append(line);
  const title=document.createElementNS(ns,'title');title.textContent=points.length?`${points.length} 个采样点，当前 ${points.at(-1).value??'采样缺口'}`:'等待两个有效采样点';svg.prepend(title);
}
