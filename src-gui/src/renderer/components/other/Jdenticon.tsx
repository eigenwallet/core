import { useEffect, useRef } from "react";
import { update } from "jdenticon";

interface JdenticonProps {
  value: string;
  size: number;
  className?: string;
  style?: React.CSSProperties;
}

/**
 * React wrapper component for jdenticon
 * Generates a unique identicon based on the provided value
 */
export default function Jdenticon({
  value,
  size,
  className,
  style,
}: JdenticonProps) {
  const svgRef = useRef<SVGSVGElement>(null);

  useEffect(() => {
    if (svgRef.current) {
      update(svgRef.current, value);
    }
  }, [value]);

  return (
    <svg
      ref={svgRef}
      width={size}
      height={size}
      data-jdenticon-value={value}
      className={className}
      style={style}
    />
  );
}

