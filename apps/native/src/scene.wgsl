struct Scene { size:vec4<f32>, phone:vec4<f32>, desktop:vec4<f32>, chat:vec4<f32>, popup:vec4<f32>, state:vec4<f32>, material:vec4<f32>, background:vec4<f32> }
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
  let border=smoothstep(-2.0,-.6,dist);
  color=mix(screen,vec3(.13,.14,.15),border);
  color=mix(color,under,smoothstep(-.2,1.,dist));
 }
 return color;
}
fn base(p:vec2<f32>)->vec3<f32>{
 var c=scene.background.rgb;
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
 var c=mix(blur(p/scene.size.xy,vec2(0.,1.)),vec3(1.),scene.material.y);
 let edge=smoothstep(-scene.material.w-.5,-.5,dist)*(1.-smoothstep(-.5,.5,dist));
 c=mix(c,vec3(.87),edge);
 return mix(c,under,smoothstep(0.,1.,dist));
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32>{
 let p=v.uv*scene.size.xy;
 var c=base(p);
 c=glass(p,scene.chat,c);
 if scene.popup.z>0. {c=glass(p,scene.popup,c);}
 return vec4(c,1.);
}
