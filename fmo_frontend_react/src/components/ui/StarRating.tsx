const STAR_PATH = 'M20.924 7.625a1.523 1.523 0 0 0-1.238-1.044l-5.051-.734-2.259-4.577a1.534 1.534 0 0 0-2.752 0L7.365 5.847l-5.051.734A1.535 1.535 0 0 0 1.463 9.2l3.656 3.563-.863 5.031a1.532 1.532 0 0 0 2.226 1.616L11 17.033l4.518 2.375a1.534 1.534 0 0 0 2.226-1.617l-.863-5.03L20.537 9.2a1.523 1.523 0 0 0 .387-1.575Z';

interface StarRatingProps {
  value: number;
  onChange: (value: number) => void;
  max?: number;
}

/** Clickable stars with the chosen rating beside them, e.g. "4/5" */
export function StarRating({ value, onChange, max = 5 }: StarRatingProps) {
  return (
    <div className="flex items-center gap-1 sm:gap-2">
      {Array.from({ length: max }, (_, index) => index + 1).map((star) => (
        <svg
          key={star}
          onClick={() => onChange(star)}
          className={`w-5 h-5 sm:w-6 sm:h-6 cursor-pointer ${
            star <= value ? 'text-yellow-300' : 'text-gray-300 dark:text-gray-500'
          }`}
          fill="currentColor"
          viewBox="0 0 22 20"
          xmlns="http://www.w3.org/2000/svg"
        >
          <path d={STAR_PATH} />
        </svg>
      ))}
      <span className="ml-2 sm:ml-4 text-lg sm:text-xl text-gray-900 dark:text-white">
        {value}/{max}
      </span>
    </div>
  );
}
