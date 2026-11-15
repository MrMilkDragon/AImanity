import {create,fromBinary,toBinary,type DescMessage,type MessageInitShape,type MessageShape} from "@bufbuild/protobuf";
import {ApiErrorSchema} from "@ai-phone/protocol/ai_phone/v1/system_pb";
export async function workbench<I extends DescMessage,O extends DescMessage>(operation:string,input:I,value:MessageInitShape<I>,output:O):Promise<MessageShape<O>>{
 const response=await fetch("/api/workbench/"+operation,{method:"POST",headers:{"content-type":"application/x-protobuf"},body:toBinary(input,create(input,value)),cache:"no-store"});const bytes=new Uint8Array(await response.arrayBuffer());if(response.status===401)location.assign("/login");if(!response.ok)throw new Error(fromBinary(ApiErrorSchema,bytes).message);return fromBinary(output,bytes);
}
