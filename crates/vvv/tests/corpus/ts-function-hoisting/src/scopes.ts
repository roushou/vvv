import { helper } from "./helpers";
export function example(helper = helper) {
  helper();
  function helper(value = 1) { return value ? helper(value - 1) : 0; }
  const capture = () => helper();
  { const helper = () => 2; helper(); }
  helper();
  return capture;
}
export function generators() {
  recur();
  function* recur(value = 1) { yield value; yield recur(value); }
}
export const arrow = () => {
  work();
  function work() { return work; }
  return work;
};
export class View {
  render() {
    work();
    function work() { return 1; }
    return work();
  }
}
export function duplicate() {
  same();
  function same() { return 1; }
  function same() { return 2; }
}
export function nested() {
  helper();
  { function helper() {} helper(); }
  helper();
}
export function overloaded() {
  helper();
  function helper(value: number): number;
  function helper(value: number) { return value; }
}
export function varBarrier() {
  helper();
  var value = 1;
  function helper() { return value; }
}
helper();
export function defaultCapture(seed = helper) {
  helper();
  function helper() { return seed; }
}
