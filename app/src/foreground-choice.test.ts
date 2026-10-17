// @vitest-environment jsdom
import {expect,it} from 'vitest';
import {takeForegroundChoice,resetForegroundChoice} from './foreground-choice';
it('requires explicit foreground authority and consumes it for one task',()=>{
 const input=document.createElement('input');input.type='checkbox';
 expect(takeForegroundChoice(input)).toBe(false);
 input.checked=true;expect(takeForegroundChoice(input)).toBe(true);expect(input.checked).toBe(false);
 expect(takeForegroundChoice(input)).toBe(false);
});
it('loading progress cannot retain a prior foreground choice',()=>{
 const input=document.createElement('input');input.checked=true;resetForegroundChoice(input);expect(input.checked).toBe(false);expect(takeForegroundChoice(null)).toBe(false);
});
