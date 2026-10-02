/** Callback documentation. */
export const arrow = <T>(value: T): T => value;
export const block = (value: number): number => { return value + 1; };
export const expression = function recurse(value: number): number {
  return value ? recurse(value - 1) : 0;
};
export const generator = function* recur(value: number) {
  yield value; yield recur(value);
};
export const asyncValue = async (value: number): Promise<number> => value;
export const literal = 42;
export const wrapped = ((value: number) => value);
export let first = (value: number) => value, second = function other() { return first; };
export class View {
  handler = ({ title }: { title: string }) => <unknown>title;
  literal = 42;
}
export function locals(input: number) {
  const local = (value: number): number => value + input;
  return local;
}
