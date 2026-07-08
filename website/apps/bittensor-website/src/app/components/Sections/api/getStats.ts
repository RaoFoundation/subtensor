export const getStats = async () => {
  const response = await fetch('/api/stats');
  const data = await response.json();

  return {
    totalStaked: data.total_stake || 0,
  };
};
