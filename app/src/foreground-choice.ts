/** Foreground authority is an explicit choice for one submitted task. */
export function takeForegroundChoice(input:HTMLInputElement|null):boolean{
 const allowed=input?.checked===true;
 if(input)input.checked=false;
 return allowed;
}
export function resetForegroundChoice(input:HTMLInputElement|null){if(input)input.checked=false;}
