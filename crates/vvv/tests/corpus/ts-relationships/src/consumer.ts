import { task as execute } from './bridge';
import * as api from './origin';
export function caller() {
    execute();
    api.work();
}
export function callbackCall(callback: () => void) { callback(); }
export function deferred() { const cb = () => execute(); }
export function nesting() { function nested() { execute(); } }
export function uncertain(receiver: Unknown) { receiver.work(); }
