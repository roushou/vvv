export const render = () => {
  child();
  function child() { return <div />; }
  return <section>{child()}</section>;
};
