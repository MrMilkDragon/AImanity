struct Scene { size:vec4<f32>, phone:vec4<f32>, desktop:vec4<f32>, chat:vec4<f32>, popup:vec4<f32>, child_popup:vec4<f32>, state:vec4<f32>, device_scale:vec4<f32>, material:vec4<f32>, background:vec4<f32>, mode_effects:vec4<f32>, agent_pointers:array<array<vec4<f32>,5>,2> }
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
 let x=d.x/(cos(angle)-d.x*sin(angle)/1800.);
 let y=d.y*(1.+x*sin(angle)/1800.);
 let local=vec2(x,y)+rect.zw*.5;
 let box=vec4(0.,0.,rect.zw);
 let scale=scene.device_scale[index];
 let bezel=5.*scale;
 let radius=select(13.,select(28.,22.,rect.z>rect.w),index==0u)*scale;
 let dist=sdf(local,box,radius);
 var color=under;
 let shadow=exp(-max(dist,0.)*.09)*.05;
 color*=1.-shadow;
 if dist<1. {
  let image_size=rect.zw-2.*bezel;
  let uv=(local-bezel)/image_size;
  var screen=textureSampleLevel(desktop,linear_sampler,uv,0.).rgb;
  if index==0u {screen=textureSampleLevel(phone,linear_sampler,uv,0.).rgb;}
  if scene.state.w>0. {
   for (var i=0u;i<5u;i+=1u) {
    let mark=scene.agent_pointers[index][i];
    if mark.z>0. {
     let d=length(local-bezel-mark.xy*image_size);
     let edge=(1.-smoothstep(7.,8.,d))*mark.z;
     let fill=(1.-smoothstep(5.,6.,d))*mark.z;
     screen=mix(screen,vec3(.98),edge);
     screen=mix(screen,vec3(.03,.09,.12),fill);
    }
   }
  }
  let border=smoothstep(-bezel-.5,-bezel+.5,dist);
  color=mix(screen,select(vec3(.0296,.0343,.0423),vec3(.00857),index==0u),border);
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
 let bend=max(a.y,b.y)+22.;
 if p.x<min(a.x,b.x)-5. || p.x>max(a.x,b.x)+5. || p.y<min(a.y,b.y)-3. || p.y>bend+3. {return under;}
 let span=b.x-a.x;
 let ratio=clamp((p.x-min(a.x,b.x))/max(abs(span),1.),0.,1.);
 let t=.5-sign(span)*sin(asin(1.-2.*ratio)/3.);
 let u=1.-t;
 let y=(a.y+4.)*u*u*u+3.*bend*u*t+(b.y+4.)*t*t*t;
 let dx=abs(6.*u*t*span);
 let dy=3.*((bend-a.y-4.)*u*u+(b.y+4.-bend)*t*t);
 let distance=abs(p.y-y)*dx/max(length(vec2(dx,dy)),.01);
 let segment=step(min(a.x,b.x)+.5,p.x)*step(p.x,max(a.x,b.x)-.5)*step(min(a.y,b.y)+4.,p.y);
 let shadow=(1.-smoothstep(2.,3.,distance))*segment*.9;
 let wire=(1.-smoothstep(.5,1.5,distance))*segment;
 let plug=min(sdf(p,vec4(a-vec2(4.,2.),8.,9.),2.),sdf(p,vec4(b-vec2(4.,2.),8.,9.),2.));
 let steel=vec3(.184,.250,.328);
 let cable=mix(mix(under,vec3(1.),shadow),steel,wire);
 let tip=mix(vec3(.776,.815,.863),steel,smoothstep(-1.1,-.4,plug));
 return mix(under,mix(cable,tip,1.-smoothstep(-.4,.6,plug)),scene.size.z);
}
fn base(p:vec2<f32>)->vec3<f32>{
 var c=scene.background.rgb;
 if scene.mode_effects.w>0. {c=copilot_background(p);}
 if scene.mode_effects.z>0. {c=goal_background(p,c);}
 if scene.state.z<.5 {
  c=device(p,scene.desktop,scene.state.y,1u,c);c=device(p,scene.phone,scene.state.x,0u,c);
 }else{
  c=device(p,scene.phone,scene.state.x,0u,c);c=device(p,scene.desktop,scene.state.y,1u,c);
 }
 if scene.size.z>0. {c=cable(p,c);}
 return c;
}
fn blur(uv:vec2<f32>,axis:vec2<f32>)->vec3<f32> {
 let step=axis*(scene.material.z/18.)/vec2<f32>(textureDimensions(glass_source));
 let pairs=array<vec2<f32>,6>(vec2(1.481490,.167710),vec2(3.456897,.131416),vec2(5.432513,.084723),vec2(7.408452,.044937),vec2(9.384821,.019609),vec2(11.361721,.007039));
 var c=textureSampleLevel(glass_source,linear_sampler,uv,0.).rgb*.089133;
 for (var i=0u;i<6u;i+=1u) {
  let offset=step*pairs[i].x;
  c+=(textureSampleLevel(glass_source,linear_sampler,uv+offset,0.).rgb+textureSampleLevel(glass_source,linear_sampler,uv-offset,0.).rgb)*pairs[i].y;
 }
 return c;
}
@fragment fn backdrop(v:Vertex)->@location(0) vec4<f32> {
 return vec4(base(v.uv*scene.size.xy),1.);
}
@fragment fn horizontal(v:Vertex)->@location(0) vec4<f32> {
 return vec4(blur(v.uv,vec2(1.,0.)),1.);
}
@fragment fn vertical(v:Vertex)->@location(0) vec4<f32> {
 return vec4(blur(v.uv,vec2(0.,1.)),1.);
}
fn glass(p:vec2<f32>,r:vec4<f32>,under:vec3<f32>)->vec3<f32>{
 let dist=sdf(p,r,scene.material.x);
 if dist>1. {return under*(1.-exp(-max(dist,0.)*.1)*.018);}
 let copilot=scene.mode_effects.w>0.;
 let backdrop=textureSampleLevel(glass_source,linear_sampler,p/scene.size.xy,0.).rgb;
 var c=mix(backdrop,vec3(select(1.,.022,copilot)),scene.material.y);
 let edge=smoothstep(-scene.material.w-.5,-.5,dist)*(1.-smoothstep(-.5,.5,dist));
 c=mix(c,select(vec3(.1384,.1812,.2462),vec3(.14),copilot),edge*select(.42,1.,copilot));
 return mix(c,under,smoothstep(0.,1.,dist));
}
fn tool_glass(p:vec2<f32>,r:vec4<f32>,under:vec3<f32>)->vec3<f32>{
 if any(p<r.xy-vec2(110.)) || any(p>r.xy+r.zw+vec2(110.,134.)) {return under;}
 let radius=min(scene.material.x,min(r.z,r.w)*.5);
 let dist=sdf(p,r,radius);
 if dist>1. {
  let d1=max(sdf(p-vec2(0.,24.),r,radius),0.)/45.;
  let d2=max(sdf(p-vec2(0.,2.),r,radius),0.)/4.;
  return under*(1.-exp(-d1*d1)*.14-exp(-d2*d2)*.039);
 }
 let copilot=scene.mode_effects.w>0.;
 let backdrop=textureSampleLevel(glass_source,linear_sampler,p/scene.size.xy,0.).rgb;
 let luminance=dot(backdrop,vec3(.2126,.7152,.0722));
 let saturated=mix(vec3(luminance),backdrop,1.05);
 var glass=mix(saturated,vec3(select(1.,.015,copilot)),scene.material.y);
 let edge=1.-smoothstep(0.,scene.material.w+.5,abs(dist+scene.material.w*.5));
 glass=mix(glass,vec3(select(.7305,.1329,copilot)),edge);
 return mix(glass,under,smoothstep(0.,1.,dist));
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
 if scene.popup.z>0. {c=mix(c,tool_glass(p,scene.popup,c),scene.size.w);}
 if scene.child_popup.z>0. {c=tool_glass(p,scene.child_popup,c);}
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
