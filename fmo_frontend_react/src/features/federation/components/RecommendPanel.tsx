import { useState, type FormEvent } from 'react';
import { api } from '../../../services/api';
import { publishNostrEvent } from '../../../services/nostr';
import { errorMessage } from '../../../utils/errors';
import { nostrNetworkTag } from '../../../utils/network';
import { Alert } from '../../../components/ui/Alert';
import { Panel } from '../../../components/ui/Panel';
import { StarRating } from '../../../components/ui/StarRating';

/** A star rating with a comment, published to Nostr as a recommendation of the federation */
export function RecommendPanel({ id, network }: { id: string; network: string | undefined }) {
  const [rating, setRating] = useState(5);
  const [comment, setComment] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [published, setPublished] = useState(false);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError(null);
    setPublished(false);

    try {
      await publishNostrEvent(() => {
        if (!id) throw new Error('Federation ID missing');
        // Without a known network the tag is left out rather than guessed
        const networkTag = nostrNetworkTag(network);

        // Kind 38000 for rating
        return {
          kind: 38000,
          tags: [
            ['d', id],
            ...(networkTag ? [['n', networkTag]] : []),
            ['k', '38173'],
          ],
          content: `[${rating}/5] ${comment}`,
        };
      }, (event) => api.publishNostrRating(event));

      setPublished(true);
      setComment('');
    } catch (err) {
      setError(`Could not publish the rating: ${errorMessage(err)}`);
    }
  };

  return (
    <Panel title="Recommend">
      <form onSubmit={handleSubmit}>
        {error && (
          <Alert level="error" message={error} />
        )}
        {published && (
          <Alert level="success" message="Your recommendation was published!" />
        )}

        <div className="mb-4">
          <StarRating value={rating} onChange={setRating} />
        </div>

        <div className="mb-4">
          <input
            type="text"
            value={comment}
            onChange={(e) => setComment(e.target.value)}
            placeholder="Comment"
            className="block w-full p-3 sm:p-4 text-sm sm:text-base text-gray-900 border border-gray-300 rounded-lg bg-white dark:bg-gray-700 dark:border-gray-600 dark:placeholder-gray-400 dark:text-white focus:ring-blue-500 focus:border-blue-500"
          />
        </div>

        <button
          type="submit"
          className="w-full px-5 py-2.5 text-sm sm:text-base text-white bg-blue-700 hover:bg-blue-800 focus:ring-4 focus:ring-blue-300 font-medium rounded-lg dark:bg-blue-600 dark:hover:bg-blue-700 dark:focus:ring-blue-800"
        >
          Rate
        </button>
      </form>
    </Panel>
  );
}
