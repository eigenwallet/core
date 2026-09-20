import { ReactNode, useState } from "react";

interface AsyncActionButtonProps<T> {
  /**
   A render prop: render your desired component accoring to the action's state.
   Start the action by calling `start_action`.
   */
  content: (actionState: AsyncActionState<T>, startAction: (arg0: AsyncAction<T>) => void) => ReactNode;
  /**
    A function called when the action returns its result.
    Use it to inspect the value without changing it.
    */
  inspect?: (arg0: T) => void;
}

interface AsyncActionState<T> {
  result: T | null
  isLoading: boolean;
}

type AsyncAction<T> = () => Promise<T>;

/**
 A generic wrapper you can use to dispatch an async action and render
 a component according to the result.
 */
export default function AsyncActionButton<T>({
  content
}: AsyncActionButtonProps<T>): ReactNode {
  const [result, setResult] = useState<T | null>(null)
  const [isLoading, setIsLoading] = useState<boolean>(false);

  const state = {
    result,
    isLoading,
  };

  const startAction = (action: AsyncAction<T>) => {
    setIsLoading(true);
    setResult(null);

    action().then(result => {
      setResult(result);
      setIsLoading(false);
    })
  }

  return (
    <>
      {content(state, startAction)}
    </>
  )
}

