struct Scene { size:vec4<f32>, phone:vec4<f32>, desktop:vec4<f32>, chat:vec4<f32>, popup:vec4<f32>, child_popup:vec4<f32>, state:vec4<f32>, device_scale:vec4<f32>, material:vec4<f32>, background:vec4<f32>, mode_effects:vec4<f32>, mode_motion:vec4<f32>, agent_pointers:array<array<vec4<f32>,5>,2> }
@group(0) @binding(0) var<uniform> scene:Scene;
@group(0) @binding(1) var phone:texture_2d<f32>;
@group(0) @binding(2) var desktop:texture_2d<f32>;
@group(0) @binding(3) var linear_sampler:sampler;
@group(0) @binding(4) var copilot_ink:texture_2d<f32>;
@group(1) @binding(0) var glass_source:texture_2d<f32>;
struct Vertex { @builtin(position) position:vec4<f32>, @location(0) uv:vec2<f32> }
struct CopilotMark {
 @builtin(position) position:vec4<f32>,
 @location(0) distance:vec2<f32>,
 @location(1) radius:f32,
 @location(2) tone:f32
}
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
fn mark_position(p:vec2<f32>)->vec4<f32> {
 return vec4(p.x/scene.size.x*2.-1.,1.-p.y/scene.size.y*2.,0.,1.);
}
@vertex fn copilot_particle(@builtin(vertex_index) vertex_index:u32,@builtin(instance_index) point:u32)->CopilotMark {
 let quad=array<vec2<f32>,6>(vec2(-1.,-1.),vec2(1.,-1.),vec2(1.,1.),vec2(-1.,-1.),vec2(1.,1.),vec2(-1.,1.));
 let size=scene.size.xy;
 let scale=min(size.x,size.y);
 let time=scene.mode_effects.x;
 let seed=f32(point)*2.399963;
 let grain=fract(f32(point)*vec2(.754877666,.569840291));
 let cell=vec2(f32(point%48u),f32(point/48u))+(grain-.5)*1.45;
 let x=cell.x/23.5-1.;
 let z=cell.y/7.5-1.;
 let phase=.43+time*1.67;
 let sway=cos(time*3.141593);
 let yaw=.24*sin(time*1.18)-.12;
 let depth=scale*(.028-.017*sway);
 let crest=pow(max(0.,1.-.66*abs(x)),1.2);
 let wave=.62*cos(phase-x*2.75+z*.46)*crest-.19*cos(phase);
 let ripple=.112*sin(phase*2.3+x*18.+z*6.);
 let scatter=.15*sin(seed+phase*2.1);
 let near=(z+1.)*.5;
 let perspective=.93+near*.11;
 let span=scale*(.214+.022*sway);
 let position=vec2(
  size.x*.5-scale*(.009+.012*sin(time*1.57+.1))+(x*cos(yaw)+z*sin(yaw)*.38)*span*perspective,
  size.y*.476+z*depth+x*depth*yaw-(wave+ripple+scatter)*scale*.066*scene.mode_motion.z);
 let sparkle=.5+.5*cos(seed+phase*1.74);
 let radius=clamp(scale/336.,.65,3.1)*(.49+near*.34+sparkle*.58);
 let shades=array<f32,5>(.18,.36,.57,.76,.91);
 var mark:CopilotMark;
 mark.distance=quad[vertex_index]*(radius+.5);
 mark.position=mark_position(position+mark.distance);
 mark.radius=radius;
 mark.tone=shades[min(4u,u32((near*.52+sparkle*.47)*5.))];
 return mark;
}
const WAVE_POINTS=array<vec2<f32>,56>(
 vec2(-70.,240.),vec2(34.,236.),vec2(99.,263.),vec2(147.,235.),vec2(212.,159.),vec2(250.,202.),vec2(309.,401.),vec2(355.,308.),vec2(398.,55.),vec2(444.,149.),vec2(513.,323.),vec2(565.,254.),vec2(672.,220.),vec2(795.,240.),
 vec2(-70.,240.),vec2(18.,244.),vec2(86.,216.),vec2(145.,244.),vec2(198.,305.),vec2(246.,274.),vec2(309.,79.),vec2(350.,171.),vec2(410.,408.),vec2(452.,319.),vec2(511.,188.),vec2(564.,223.),vec2(664.,264.),vec2(795.,240.),
 vec2(-70.,240.),vec2(43.,238.),vec2(91.,263.),vec2(148.,237.),vec2(211.,194.),vec2(250.,221.),vec2(307.,349.),vec2(352.,289.),vec2(410.,109.),vec2(452.,191.),vec2(520.,286.),vec2(563.,255.),vec2(656.,227.),vec2(795.,240.),
 vec2(-70.,240.),vec2(33.,241.),vec2(90.,225.),vec2(154.,243.),vec2(216.,274.),vec2(251.,257.),vec2(310.,150.),vec2(350.,199.),vec2(412.,356.),vec2(449.,294.),vec2(513.,206.),vec2(563.,230.),vec2(661.,247.),vec2(795.,240.));
@vertex fn copilot_wave(@builtin(vertex_index) vertex_index:u32,@builtin(instance_index) wave:u32)->CopilotMark {
 let step=vertex_index/2u;
 let segment=min(step/24u,5u);
 let t=f32(step-segment*24u)/24.;
 let u=1.-t;
 let base=wave*14u;
 let end=base+segment*2u+3u;
 var a=WAVE_POINTS[base];
 var b=WAVE_POINTS[base+1u];
 if segment>0u {
  a=WAVE_POINTS[end-2u];
  b=2.*a-WAVE_POINTS[end-3u];
 }
 let c=WAVE_POINTS[end-1u];
 let d=WAVE_POINTS[end];
 let activity=u32(scene.mode_effects.w)-1u;
 let duration=array<f32,3>(7.,2.7,2.2)[activity];
 let delay=array<f32,4>(0.,.8,1.6,1.1)[wave];
 let traversal=1.-abs(2.*fract((scene.mode_effects.x+delay)/(2.*duration))-1.);
 let drift=.5-.5*cos(select(traversal,1.-traversal,wave%2u==1u)*3.141593);
 let height=array<f32,3>(.5,1.28,1.65)[activity]*mix(.69,1.14,drift);
 let tone=array<f32,3>(.28,.43,.48)[activity]*array<f32,4>(1.,.7,.5,.45)[wave];
 let scale=scene.chat.zw/vec2(720.,480.)*vec2(1.,height);
 let xyz=a*u*u*u+3.*b*u*u*t+3.*c*u*t*t+d*t*t*t;
 let center=(xyz-vec2(0.,240.))*scale+scene.chat.xy+vec2(mix(-.04,.04,drift)*scene.chat.z,scene.chat.w*.5);
 let slope=(3.*u*u*(b-a)+6.*u*t*(c-b)+3.*t*t*(d-c))*scale;
 let normal=normalize(vec2(-slope.y,slope.x));
 let side=select(-1.,1.,vertex_index%2u==1u);
 var mark:CopilotMark;
 mark.position=mark_position(center+side*normal);
 mark.distance=vec2(side,0.);
 mark.radius=.5;
 mark.tone=tone;
 return mark;
}
@fragment fn copilot_stroke(mark:CopilotMark)->@location(0) vec4<f32> {
 let coverage=clamp(mark.radius+.5-length(mark.distance),0.,1.);
 return vec4(1.,1.,1.,mark.tone*coverage);
}
fn copilot_background(p:vec2<f32>)->vec3<f32> {
 let size=scene.size.xy;
 let uv=p/size;
 let scale=min(size.x,size.y);
 let radial=1.-clamp(length((uv-vec2(.48,.02))/vec2(.77,1.08))/.74,0.,1.);
 let gradient=mix(mix(.027,.0156,uv.y),.039,radial);
 let y=(uv.y-.585)/.33;
 let lattice=vec2(p.x/scale)+vec2(2.6,-2.6)*y;
 let directions=abs(fract(lattice*6.5+.5)-vec2(.5));
 let distance=min(directions.x,directions.y)/length(vec2(6.5/scale,16.9/(.33*size.y)));
 let intensity=max(0.,min(y/.4,(1.-y)/.6))*.048;
 let grid=line_alpha(distance,.72,1.)*intensity;
 let sway=cos(scene.mode_effects.x*3.141593);
 let drag=-.009-.012*sin(scene.mode_effects.x*1.57+.1);
 let offset=(p-vec2(.5,.746)*size-vec2(drag*scale,0.))/scale;
 let stretch=vec2(.225+.021*sway,.026-.007*sway);
 let shadow_distance=offset/stretch;
 let shadow=(1.-smoothstep(.02,1.,dot(shadow_distance,shadow_distance)))*.58;
 let points=textureSampleLevel(copilot_ink,linear_sampler,uv,0.).r;
 return vec3(mix(mix(gradient,.46208,grid)*(1.-shadow),.93869,points*(.23+.77*points)));
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
 let reveal=1.-smoothstep(scene.device_scale.w-.005,scene.device_scale.w,t);
 let segment=step(min(a.x,b.x)+.5,p.x)*step(p.x,max(a.x,b.x)-.5)*step(min(a.y,b.y)+4.,p.y)*reveal;
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
 if scene.mode_effects.z>0. {c=goal_background(p,c);}
 if scene.mode_effects.w>0. {c=copilot_background(p);}
 if p.y<scene.device_scale.z {return c;}
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
 let backdrop=select(textureSampleLevel(glass_source,linear_sampler,p/scene.size.xy,0.).rgb,under,copilot);
 var c=mix(backdrop,vec3(select(1.,.015,copilot)),scene.material.y);
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
 let backdrop=textureSampleLevel(glass_source,linear_sampler,p/scene.size.xy,0.).rgb;
 let luminance=dot(backdrop,vec3(.2126,.7152,.0722));
 let saturated=mix(vec3(luminance),backdrop,1.05);
 var glass=mix(saturated,vec3(1.),scene.material.y);
 let edge=1.-smoothstep(0.,scene.material.w+.5,abs(dist+scene.material.w*.5));
 glass=mix(glass,vec3(.7305),edge);
 return mix(glass,under,smoothstep(0.,1.,dist));
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
fn liberty_glow(p:vec2<f32>,under:vec3<f32>)->vec3<f32> {
 let axes=min(p,scene.size.xy-p);
 let edge=min(axes.x,axes.y);
 if edge>=76. {return under;}
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
 return mix(mix(under,palette,halo),mix(palette,vec3(1.),reflection),rim);
}
@fragment fn fragment(v:Vertex)->@location(0) vec4<f32>{
 let p=v.uv*scene.size.xy;
 var c=base(p);
 if p.y>=scene.device_scale.z {c=glass(p,scene.chat,c);}
 if p.y>=scene.device_scale.z && scene.mode_effects.w>0. && sdf(p,scene.chat,scene.material.x)<0. {
  let x=(p.x-scene.chat.x)/scene.chat.z;
  let ink=textureSampleLevel(copilot_ink,linear_sampler,p/scene.size.xy,0.).g*clamp(min(x,1.-x)/.1,0.,1.);
  c=mix(c,vec3(.90466),ink*(.23+.77*ink));
 }
 if scene.popup.z>0. && scene.size.w>0. {c=mix(c,tool_glass(p,scene.popup,c),scene.size.w);}
 if scene.child_popup.z>0. {c=tool_glass(p,scene.child_popup,c);}
 if scene.mode_effects.y>0. {c=liberty_glow(p,c);}
 return vec4(c,1.);
}
