/** Selected wrapper travels. */
export class Widget { value() { return 42; } }
export function overloaded(value: string): string;
export function overloaded(value: number): number;
export function overloaded(value: string | number) { return value; }
