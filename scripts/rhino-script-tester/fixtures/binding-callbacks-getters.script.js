// Probe: callback getters across two authentication visits. Safe to delete.
// The runner must use RESUBMIT=1 to submit visit one defaults unchanged.
function describe(v){var r={type:typeof v,string:String(v)};try{r.array=Array.isArray(v);}catch(e){}try{r.keys=Object.keys(v);}catch(e){}try{r.length=v.length;}catch(e){}try{r.size=v.size();}catch(e){}return r;}
if(callbacks.isEmpty()){
try{callbacksBuilder.stringAttributeInputCallback("attr", "Prompt", "value", true);}catch(e){}
try{callbacksBuilder.choiceCallback("Prompt", ["one","two"], 0, false);}catch(e){}
try{callbacksBuilder.nameCallback("Name", "Ada");}catch(e){}
try{callbacksBuilder.passwordCallback("Password", false);}catch(e){}
try{callbacksBuilder.hiddenValueCallback("hidden", "value");}catch(e){}
try{callbacksBuilder.textInputCallback("Prompt", "default");}catch(e){}
try{callbacksBuilder.numberAttributeInputCallback("num", "Number", 7, true);}catch(e){}
try{callbacksBuilder.booleanAttributeInputCallback("bool", "Boolean", true, true);}catch(e){}
try{callbacksBuilder.confirmationCallback("Continue?", 0, ["yes","no"], 0);}catch(e){}
try{callbacksBuilder.languageCallback("en", "US");}catch(e){}
try{callbacksBuilder.idPCallback("provider", "client", "https://example.com", ["openid"], "nonce", "", "", [], false);}catch(e){}
try{callbacksBuilder.validatedPasswordCallback("Password", false, {}, false, []);}catch(e){}
try{callbacksBuilder.validatedUsernameCallback("Username", {}, false, []);}catch(e){}
try{callbacksBuilder.httpCallback("auth", "nego", "");}catch(e){}
try{callbacksBuilder.x509CertificateCallback("certificate", "prompt");}catch(e){}
try{callbacksBuilder.consentMappingCallback({}, "Consent", true);}catch(e){}
try{callbacksBuilder.deviceProfileCallback(true, true, "Device");}catch(e){}
try{callbacksBuilder.kbaCreateCallback("Question", ["Q"], true);}catch(e){}
try{callbacksBuilder.selectIdPCallback({});}catch(e){}
try{callbacksBuilder.termsAndConditionsCallback("v1", "terms", "2026-01-01");}catch(e){}
outcome="ok";
}else{
var r=[];
try{var x=callbacks.getStringAttributeInputCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getStringAttributeInputCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getStringAttributeInputCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getChoiceCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getChoiceCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getChoiceCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getNameCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getNameCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getNameCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getPasswordCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getPasswordCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getPasswordCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getHiddenValueCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getHiddenValueCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getHiddenValueCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getTextInputCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getTextInputCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getTextInputCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getNumberAttributeInputCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getNumberAttributeInputCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getNumberAttributeInputCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getBooleanAttributeInputCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getBooleanAttributeInputCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getBooleanAttributeInputCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getConfirmationCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getConfirmationCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getConfirmationCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getLanguageCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getLanguageCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getLanguageCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getIdpCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getIdpCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getIdpCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getValidatedPasswordCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getValidatedPasswordCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getValidatedPasswordCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getValidatedUsernameCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getValidatedUsernameCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getValidatedUsernameCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getHttpCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getHttpCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getHttpCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getX509CertificateCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getX509CertificateCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getX509CertificateCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getConsentMappingCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getConsentMappingCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getConsentMappingCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getDeviceProfileCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getDeviceProfileCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getDeviceProfileCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getKbaCreateCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getKbaCreateCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getKbaCreateCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getSelectIdPCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getSelectIdPCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getSelectIdPCallbacks",ok:false,error:String(e)});}
try{var x=callbacks.getTermsAndConditionsCallbacks(); var first=(x&&x.length)?x[0]:null; r.push({name:"getTermsAndConditionsCallbacks",ok:true,list:describe(x),first:first===null?null:describe(first)});}catch(e){r.push({name:"getTermsAndConditionsCallbacks",ok:false,error:String(e)});}
try{r.push({name:"isEmpty",ok:true,value:callbacks.isEmpty()});}catch(e){r.push({name:"isEmpty",ok:false,error:String(e)});}
callbacksBuilder.hiddenValueCallback("result",JSON.stringify(r)); outcome="ok";
}
