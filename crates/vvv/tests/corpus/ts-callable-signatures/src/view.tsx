export const render = ({ title }: { title: string }) => <div>{title}</div>;
export class View {
  handler = (title: string) => <div>{title}</div>;
}
