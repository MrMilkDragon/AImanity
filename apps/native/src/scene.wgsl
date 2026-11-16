struct Scene { size:vec4<f32>, phone:vec4<f32>, desktop:vec4<f32>, chat:vec4<f32>, popup:vec4<f32>, child_popup:vec4<f32>, state:vec4<f32>, material:vec4<f32>, background:vec4<f32>, mode_effects:vec4<f32>, agent_pointers:array<array<vec4<f32>,5>,2> }
@group(0) @binding(0) var<uniform> scene:Scene;
@group(0) @binding(1) var phone:texture_2d<f32>;
@group(0) @binding(2) var desktop:texture_2d<f32>;
@group(0) @binding(3) var linear_sampler:sampler;
@group(1) @binding(0) var glass_source:texture_2d<f32>;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
@vertex fn vertex(@builtin(vertex_index) index:u32)->Vertex {
 let v=array<vec2<f32>,3>(vec2(-1.,-1.),vec2(3.,-1.),vec2(-1.,3.));
 var out:Vertex;out.position=vec4(v[index],0.,1.);out.uv=vec2((v[index].x+1.)*.5,(1.-v[index].y)*.5);return out;
}
fn sdf(p:vec2<f32>,r:vec4<f32>,radius:f32)->f32 {
 let q=abs(p-r.xy-r.zw*.5)-r.zw*.5+vec2(radius);return length(max(q,vec2(0.)))+min(max(q.x,q.y),0.)-radius;
}
fn device(p:vec2<f32>,rect:vec4<f32>,angle:f32,index:u32,under:vec3<f32>)->vec3<f32> {
 let d=p-rect.xy-rect.zw*.5;
 let x=d.x/(cos(angle)-d.x*sin(angle)/1000.);
 let y=d.y*(1.+x*sin(angle)/1000.);
 let local=vec2(x,y)+rect.zw*.5;
 let box=vec4(0.,0.,rect.zw);
 let radius=select(11.,21.,index==0u);
 let dist=sdf(local,box,radius);
 var color=under;
 let shadow=exp(-max(dist,0.)*.11)*.13;
 color*=1.-shadow;
 if dist<1. {
  let uv=local/rect.zw;
  var screen=textureSampleLevel(desktop,linear_sampler,uv,0.).rgb;
  if index==0u {screen=textureSampleLevel(phone,linear_sampler,uv,0.).rgb;}
  if scene.state.w>0. {
   for (var i=0u;i<5u;i+=1u) {
    let mark=scene.agent_pointers[index][i];
    if mark.z>0. {
     let d=length(local-mark.xy*rect.zw);
     let edge=(1.-smoothstep(7.,8.,d))*mark.z;
     let fill=(1.-smoothstep(5.,6.,d))*mark.z;
     screen=mix(screen,vec3(.98),edge);
     screen=mix(screen,vec3(.03,.09,.12),fill);
    }
   }
  }
  let border=smoothstep(-2.0,-.6,dist);
  color=mix(screen,vec3(.13,.14,.15),border);
  color=mix(color,under,smoothstep(-.2,1.,dist));
 }
 return color;
}
fn orbit(p:vec2<f32>,origin:vec2<f32>,radius:f32,phase:f32)->f32 {
 let pos=p-origin;
 let dist=length(pos);
 let turn=atan2(pos.y,pos.x)/6.283185;
 let guide=1.-smoothstep(.35,1.05,abs(dist-radius));
 let tick=(1.-smoothstep(.08,.23,abs(fract((turn+phase)*76.)-.5)))*(1.-smoothstep(1.,2.5,abs(dist-radius+11.)));
 let sweep=smoothstep(.06,.10,fract(turn+phase))*(1.-smoothstep(.28,.33,fract(turn+phase)))*(1.-smoothstep(.4,1.6,abs(dist-radius+22.)));
 let node=1.-smoothstep(2.,3.4,length(pos-(radius-22.)*vec2(cos(phase*(-6.283185)),sin(phase*(-6.283185)))));
 return guide*.19+tick*.33+sweep*.62+node*.78;
}
fn goal_background(p:vec2<f32>,under:vec3<f32>)->vec3<f32> {
 let size=scene.size.xy;
 let uv=p/size;
 let wash=exp(-length((uv-vec2(.22,.46))*vec2(3.,2.5))*.94);
 let rose=exp(-length((uv-vec2(.82,.71))*vec2(2.8,3.0)));
 let c=mix(mix(under,vec3(.66,.58,.8),wash*.23),vec3(.84,.65,.49),rose*.16);
 let scale=min(size.x,size.y)/800.;
 let t=scene.mode_effects.z-1.;
 let left=vec2(.18,.54)*size;
 let right=vec2(.96,.67)*size;
 let ring=orbit(p,left,306.*scale,t/48.)+orbit(p,left,191.*scale,-t/33.)+orbit(p,right,210.*scale,.196-t/56.);
 let diameter=length((p-left)/scale);
 let outer=1.-smoothstep(.25,1.25,abs(diameter-336.));
 return mix(c,mix(vec3(.43,.34,.64),vec3(.73,.43,.22),uv.x),min(1.,ring*.52+outer*.13));
}
fn copilot_background(p:vec2<f32>)->vec3<f32> {
 let size=scene.size.xy;
 let uv=p/size;
 let time=scene.mode_effects.x;
 let tilt=(uv.x-.5)*.028*cos(time*.73);
 let phase=time*2.75-(uv.x-.5)*5.;
 let rise=(1.-abs(uv.x-.5)*1.52)*.072;
 let amplitude=.61+scene.mode_effects.w*.13;
 let crest=cos(phase)*rise*amplitude+sin(phase*1.89)*.011;
 let row=(uv.y-.49+crest-tilt)*size.y/min(size.x,size.y)*127.;
 let col=(uv.x-.5)*56.+floor(row+.5)*.12;
 let dot_distance=length(vec2((fract(col+.5)-.5)*size.x/56.,(fract(row+.5)-.5)*min(size.x,size.y)/127.));
 let dot=(1.-smoothstep(.65,1.7,dot_distance))*(1.-smoothstep(7.4,9.2,abs(row)))*smoothstep(.025,.08,min(uv.x,1.-uv.x));
 let sparkle=.62+sin(floor(col+.5)*2.399+floor(row+.5)*.57+time*4.1)*.27;
 let top=exp(-length((uv-vec2(.48,.02))*vec2(1.7,1.4))*3.);
 let floor_grid_a=fract((uv.x+(uv.y-.58)*1.72)*18.);
 let floor_grid_b=fract((uv.x-(uv.y-.58)*1.72)*18.);
 let grid=max(1.-smoothstep(.016,.033,min(floor_grid_a,1.-floor_grid_a)),1.-smoothstep(.016,.033,min(floor_grid_b,1.-floor_grid_b)));
 let plane=smoothstep(.58,.70,uv.y)*(1.-smoothstep(.72,.90,uv.y));
 return vec3(.013+.022*top+grid*plane*.019)+vec3(dot*sparkle*.64);
}
fn cable(p:vec2<f32>,under:vec3<f32>)->vec3<f32> {
 let a=vec2(scene.phone.x+scene.phone.z*.5,scene.phone.y+scene.phone.w);
 let b=vec2(scene.desktop.x+scene.desktop.z*.5,scene.desktop.y+scene.desktop.w);
 let t=clamp((p.x-a.x)/max(b.x-a.x,1.),0.,1.);
 let shelf=max(a.y,b.y)+16.;
 let y=mix(mix(a.y,shelf,smoothstep(0.,.24,t)),b.y,smoothstep(.76,1.,t));
 let wire=(1.-smoothstep(1.,2.,abs(p.y-y)))*step(a.x,p.x)*step(p.x,b.x);
 let plug_a=1.-smoothstep(-.45,.55,sdf(p,vec4(a-vec2(3.5,2.),7.,8.),2.));
 let plug_b=1.-smoothstep(-.45,.55,sdf(p,vec4(b-vec2(3.5,2.),7.,8.),2.));
 return mix(under,vec3(.22,.24,.27),max(wire,max(plug_a,plug_b))*scene.size.z);
}
fn base(p:vec2<f32>)->vec3<f32>{
 var c=scene.background.rgb;
 if scene.mode_effects.w>0. {c=copilot_background(p);}
 if scene.mode_effects.z>0. {c=goal_background(p,c);}
 if scene.size.z>0. {c=cable(p,c);}
 if scene.state.z<.5 {
  c=device(p,scene.desktop,scene.state.y,1u,c);c=device(p,scene.phone,scene.state.x,0u,c);
 }else{
  c=device(p,scene.phone,scene.state.x,0u,c);c=device(p,scene.desktop,scene.state.y,1u,c);
 }
 return c;
}
fn blur(uv:vec2<f32>,axis:vec2<f32>)->vec3<f32> {
 let step=axis*scene.material.z/1.75/scene.size.xy;
 var c=textureSampleLevel(glass_source,linear_sampler,uv,0.).rgb*.227027;
 c+=(textureSampleLevel(glass_source,linear_sampler,uv+step*1.384615,0.).rgb+textureSampleLevel(glass_source,linear_sampler,uv-step*1.384615,0.).rgb)*.316216;
 c+=(textureSampleLevel(glass_source,linear_sampler,uv+step*3.230769,0.).rgb+textureSampleLevel(glass_source,linear_sampler,uv-step*3.230769,0.).rgb)*.070270;
 return c;
}
@fragment fn backdrop(v:Vertex)->@location(0) vec4<f32> {
 return vec4(base(v.uv*scene.size.xy),1.);
}
@fragment fn horizontal(v:Vertex)->@location(0) vec4<f32> {
 return vec4(blur(v.uv,vec2(1.,0.)),1.);
}
fn glass(p:vec2<f32>,r:vec4<f32>,under:vec3<f32>)->vec3<f32>{
 let dist=sdf(p,r,scene.material.x);
 if dist>1. {return under*(1.-exp(-max(dist,0.)*.1)*.018);}
 let copilot=scene.mode_effects.w>0.;
 var c=mix(blur(p/scene.size.xy,vec2(0.,1.)),vec3(select(1.,.022,copilot)),scene.material.y);
 let edge=smoothstep(-scene.material.w-.5,-.5,dist)*(1.-smoothstep(-.5,.5,dist));
 c=mix(c,vec3(select(.87,.14,copilot)),edge);
 return mix(c,under,smoothstep(0.,1.,dist));
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32>{
 let p=v.uv*scene.size.xy;
 var c=base(p);
 c=glass(p,scene.chat,c);
 if scene.mode_effects.w>0. && sdf(p,scene.chat,scene.material.x)<0. {
  let uv=(p-scene.chat.xy)/scene.chat.zw;
  let t=scene.mode_effects.x;
  let envelope=sin(uv.x*3.14159)*sin(uv.x*3.14159);
  let amplitude=.035+.033*scene.mode_effects.w;
  var waves=0.;
  for(var i=0.;i<4.;i+=1.) {
   let y=.52+sin(uv.x*(11.+i*.8)+i*.72+t*(.54+scene.mode_effects.w*.2))*amplitude*envelope;
   waves+=1.-smoothstep(.3,1.5,abs(uv.y-y)*scene.chat.w);
  }
  c+=vec3(waves*envelope*.16);
 }
 if scene.popup.z>0. {c=glass(p,scene.popup,c);}
 if scene.child_popup.z>0. {c=glass(p,scene.child_popup,c);}
 if scene.mode_effects.y>0. {
  let uv=p/scene.size.xy;
  let edge=min(min(p.x,scene.size.x-p.x),min(p.y,scene.size.y-p.y));
  let phase=(atan2(uv.y-.5,uv.x-.5)/6.283185-scene.mode_effects.x/14.)*6.283185;
  let palette=.55+.42*cos(vec3(phase-.2,phase-2.3,phase-4.4));
  let width=(1.-smoothstep(0.,1.5,edge))*.74+exp(-edge*.044)*.30;
  c=mix(c,palette,clamp(scene.mode_effects.y*width,0.,1.));
 }
 return vec4(c,1.);
}
