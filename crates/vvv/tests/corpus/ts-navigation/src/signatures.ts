/** Preserve the object return type and generic constraint. */
export function build<T extends { value: number }>(input: T): { value: T } {
    return { value: input };
}
export const factory = () => build({ value: 1 });
