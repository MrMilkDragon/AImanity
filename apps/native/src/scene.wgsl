struct Scene { size:vec4<f32>, chat:vec4<f32>, popup:vec4<f32>, child_popup:vec4<f32>, radii_and_clip:vec4<f32>, material:vec4<f32>, background:vec4<f32>, mode_effects:vec4<f32>, mode_motion:vec4<f32> }
@group(0) @binding(0) var<uniform> scene:Scene;
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


struct Device { bounds:vec4<f32>, shape:vec4<f32>, markers:array<vec4<f32>,5> }
@group(1) @binding(1) var<uniform> device:Device;
@vertex fn device_vertex(@builtin(vertex_index) i:u32)->Vertex {
 let corners=array<vec2<f32>,6>(vec2(0.,0.),vec2(1.,0.),vec2(0.,1.),vec2(0.,1.),vec2(1.,0.),vec2(1.,1.));
 let p=device.bounds.xy-vec2(30.)+corners[i]*(device.bounds.zw+vec2(60.));
 let uv=p/scene.size.xy;
 var out:Vertex;out.position=vec4(uv*vec2(2.,-2.)+vec2(-1.,1.),0.,1.);out.uv=uv;return out;
}
@fragment fn device_pixel(v:Vertex)->@location(0) vec4<f32> {
 let p=v.uv*scene.size.xy;
 if p.y<scene.radii_and_clip.z {discard;}
 let r=device.bounds;let radius=device.shape.x;let bezel=radius/3.;
 let edge=sdf(p,r,radius);
 let coverage=1.-smoothstep(-.55,.55,edge);
 let sd=max(0.,sdf(p-vec2(0.,6.),r,radius));
 let shadow=.15*exp(-sd*sd/245.);
 let inner=r.zw-2.*bezel;let uv=(p-r.xy-bezel)/inner;
 let image=textureSampleLevel(glass_source,linear_sampler,uv,0.).rgb;
 var screen=mix(vec3(.84,.87,.89),image,device.shape.y);
 for(var i=0u;i<5u;i+=1u) {
  let mark=device.markers[i];
  if mark.z>0. {let d=length((uv-mark.xy)*inner);screen=mix(screen,vec3(1.),mark.z*(1.-smoothstep(6.,7.,d)));screen=mix(screen,vec3(.017,.025,.04),mark.z*(1.-smoothstep(4.,5.,d)));}
 }
 let frame=mix(vec3(.026,.032,.039),screen,1.-smoothstep(-bezel-.5,-bezel+.5,edge));
 return vec4(frame*coverage,coverage+shadow*(1.-coverage));
}

fn line_alpha(distance:f32,width:f32,pixel:f32)->f32 {
 return 1.-smoothstep(max(0.,(width-pixel)*.5),(width+pixel)*.5,distance);
}
fn line_distance(p:vec2<f32>,a:vec2<f32>,b:vec2<f32>)->f32 {
 let d=b-a;
 return length(p-a-d*clamp(dot(p-a,d)/dot(d,d),0.,1.));
}
fn engraving(p:vec2<f32>,pixel:f32)->f32 {
 let points=array<vec2<f32>,20>(
  vec2(-40.,610.),vec2(193.,610.),vec2(557.,303.),vec2(955.,303.),vec2(1094.,203.),vec2(1480.,203.),
  vec2(-40.,704.),vec2(181.,704.),vec2(521.,421.),vec2(1014.,421.),vec2(1190.,562.),vec2(1485.,562.),
  vec2(-20.,564.),vec2(155.,564.),vec2(532.,233.),vec2(862.,233.),
  vec2(1447.,619.),vec2(1198.,619.),vec2(1070.,513.),vec2(920.,513.));
 var distance=1e4;
 for(var i=1u;i<20u;i+=1u) {
  if i!=6u && i!=12u && i!=16u {
   let a=points[i-1u];
   let b=points[i];
   if all(p>=min(a,b)-vec2(pixel)) && all(p<=max(a,b)+vec2(pixel)) {
    distance=min(distance,line_distance(p,a,b));
   }
  }
 }
 return line_alpha(distance,.85,pixel)*.29;
}
fn guide(distance:f32,radii:vec3<f32>,pixel:f32)->f32 {
 let d=abs(vec3(distance)-radii);
 return line_alpha(min(d.x,min(d.y,d.z)),.8,pixel)*.29;
}
fn orbit(p:vec2<f32>,radii:vec3<f32>,phase:f32,node:vec2<f32>,node_radius:f32,pixel:f32)->f32 {
 let distance=length(p);
 if distance>radii.x+4. || distance<min(radii.y,radii.z)-4. {return 0.;}
 let angle=fract(atan2(p.y,p.x)/6.283185-phase)*360.;
 let tick=select(0.,line_alpha(abs(distance-radii.x),4.,pixel),fract(angle/5.)<.2);
 let swept=angle<69. || (angle>=87. && angle<98.) || (angle>=196. && angle<232.);
 let sweep=select(0.,line_alpha(abs(distance-radii.y),1.8,pixel),swept);
 let dash=fract(angle/20.)*20.;
 let thread=select(0.,line_alpha(abs(distance-radii.z),1.,pixel),radii.y!=radii.z && (dash<6. || (dash>=8. && dash<10.)));
 let turn=phase*6.283185;
 let rotated=vec2(node.x*cos(turn)-node.y*sin(turn),node.x*sin(turn)+node.y*cos(turn));
 let point=1.-smoothstep(node_radius-pixel*.5,node_radius+pixel*.5,length(p-rotated));
 return max(max(tick*.54,sweep*.61),max(thread*.36,point*.8));
}
fn goal_background(p:vec2<f32>,under:vec3<f32>)->vec3<f32> {
 let size=scene.size.xy;
 let uv=p/size;
 let lavender=1.-clamp(length((uv-vec2(.28,.46))/vec2(1.018,.764))/.57,0.,1.);
 let sand=1.-clamp(length((uv-vec2(.78,.76))/vec2(1.103,1.075))/.48,0.,1.);
 let c=mix(mix(under,vec3(.79130,.59720,.46778),sand*.286),vec3(.74540,.68669,.85499),lavender*.431);
 let scale=max(size.x/1440.,size.y/900.);
 let q=(p-size*.5)/scale+vec2(720.,450.);
 let t=scene.mode_effects.z-1.;
 let left=(q-vec2(291.,485.))/1.29;
 let right=(q-vec2(1370.,544.))/1.18;
 let left_pixel=1./(scale*1.29);
 let right_pixel=1./(scale*1.18);
 let guides=max(guide(length(left),vec3(326.,294.,160.),left_pixel),guide(length(right),vec3(218.,137.,137.),right_pixel));
 let rings=max(max(orbit(left,vec3(294.,272.,260.),t/48.,vec2(272.,0.),3.,left_pixel),
  orbit(left,vec3(195.,171.,171.),-t/33.,vec2(0.,-171.),3.5,left_pixel)),
  orbit(right,vec3(218.,207.,189.),(t+11.)/56.,vec2(-207.,0.),3.,right_pixel));
 let position=clamp((q.x/1440.+1.-q.y/900.)*.5,0.,1.);
 let purple=vec3(.24228,.13843,.44520);
 let steel=vec3(.21586,.31399,.49102);
 let copper=vec3(.53328,.26225,.10946);
 let gold=vec3(.89627,.46778,.22323);
 let metal=select(mix(purple,steel,position/.42),
  select(mix(steel,copper,(position-.42)/.38),mix(copper,gold,(position-.8)/.2),position>=.8),position>=.42);
 return mix(c,metal,max(max(guides,rings),engraving(q,1./scale)));
}
fn base(p:vec2<f32>)->vec3<f32>{
 let uv=p/scene.size.xy;
 let cool=exp(-dot((uv-vec2(.66,.35))*vec2(2.2,1.8),(uv-vec2(.66,.35))*vec2(2.2,1.8)));
 let warm=exp(-dot((uv-vec2(.08,.95))*vec2(2.8,2.),(uv-vec2(.08,.95))*vec2(2.8,2.)));
 var c=mix(mix(scene.background.rgb,vec3(.81,.865,.90),cool*.70),vec3(.94,.897,.84),warm*.64);
 let grid=length((fract((p-scene.radii_and_clip.xy)/scene.radii_and_clip.w+.5)-.5)*scene.radii_and_clip.w);
 c=mix(c,vec3(.44,.52,.58),(.095*(1.-smoothstep(.45,1.35,grid)))*smoothstep(42.,86.,p.y));
 if scene.mode_effects.z>0. {c=goal_background(p,c);}
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
fn over(top:vec4<f32>,bottom:vec4<f32>)->vec4<f32>{return top+bottom*(1.-top.a);}
fn pane(p:vec2<f32>,r:vec4<f32>,opacity:f32)->vec4<f32>{
 let radius=min(scene.material.x,min(r.z,r.w)*.5);
 let d=sdf(p,r,radius);
 if d>40. {return vec4(0.);}
 let ds=max(sdf(p-vec2(0.,8.),r,radius),0.);
 let shadow=.08*exp(-ds*ds/356.)+.025*exp(-max(d,0.)*.6);
 let a=1.-smoothstep(-.5,.5,d);
 let sample=textureSampleLevel(glass_source,linear_sampler,p/scene.size.xy,0.).rgb;
 var color=mix(sample,vec3(1.),scene.material.y);
 let edge=smoothstep(-scene.material.w-0.8,-.2,d);
 color=mix(color,vec3(.39,.45,.52),edge*.43);
 return vec4(color*a,a+shadow*(1.-a))*opacity;
}
fn liberty_falloff(point:f32)->f32 {
 if point<=16. {return mix(1.,.466667,point/16.);}
 if point<=40. {return mix(.466667,.133333,(point-16.)/24.);}
 return .133333*(1.-clamp((point-40.)/36.,0.,1.));
}
fn liberty_color(turn:f32)->vec3<f32> {
 let palette=array<vec3<f32>,7>(
  vec3(.12477,.69387,.83880),vec3(.18447,.22697,.91310),vec3(.43415,.18782,.81485),
  vec3(.84687,.31399,.52100),vec3(1.,.53948,.29177),vec3(.16513,.72306,.61721),vec3(.12477,.69387,.83880));
 let stops=array<f32,7>(0.,.18,.35,.52,.68,.84,1.);
 for(var i=1u;i<6u;i+=1u) {
  if turn<stops[i] {return mix(palette[i-1u],palette[i],(turn-stops[i-1u])/(stops[i]-stops[i-1u]));}
 }
 return mix(palette[5u],palette[6u],(turn-.84)/.16);
}
fn liberty_glow(p:vec2<f32>)->vec4<f32> {
 let axes=min(p,scene.size.xy-p);
 let edge=min(axes.x,axes.y);
 if edge>=76. {return vec4(0.);}
 let distance=length(p-scene.size.xy)/length(scene.size.xy);
 let reveal=scene.mode_motion.y;
 let opacity=scene.mode_effects.y*(1.-smoothstep(max(0.,reveal-.1),reveal,distance));
 let offset=p-scene.size.xy*.5;
 let turn=fract(atan2(offset.x,-offset.y)/6.283185-1./12.-scene.mode_motion.x/14.);
 let palette=liberty_color(turn);
 let horizontal=liberty_falloff(axes.x);
 let vertical=liberty_falloff(axes.y);
 let halo=(1.-(1.-vertical)*(1.-horizontal))*.6*opacity;
 let rim=(1.-smoothstep(1.,2.,edge))*.85*opacity;
 let reflection=max(0.,max(1.-turn/.16,(turn-.82)/.18))*.65;
 return vec4(palette*halo*(1.-rim)+mix(palette,vec3(1.),reflection)*rim,halo+rim*(1.-halo));
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32>{
 let p=v.uv*scene.size.xy;
 var c=pane(p,scene.chat,1.);
 if scene.popup.z>0. && scene.size.w>0. {c=over(pane(p,scene.popup,scene.size.w),c);}
 if scene.child_popup.z>0. && scene.background.w>0. {c=over(pane(p,scene.child_popup,scene.background.w),c);}
 if scene.mode_effects.y>0. {c=over(liberty_glow(p),c);}
 return c;
}
