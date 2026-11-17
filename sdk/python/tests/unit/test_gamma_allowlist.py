import pytest

from bittensor.intents.batch import Batch
from bittensor.intents.emission_weights import SetSubnetEmissionWeights
from bittensor.intents.plan import Policy


# Replacing the complete target map can remove a later target and redirect its emission.
# PROTECTED_REGRESSION: changes require explicit human approval.
@pytest.mark.parametrize("gamma", [{}, {7: 0}, {7: "5%"}])
def test_gamma_replacement_requires_unrestricted_subnet_policy(gamma):
    intent = SetSubnetEmissionWeights(netuid=1, slots=["owner"], values=["18%"], gamma=gamma)
    for operation in (intent, Batch([intent])):
        assert operation.affects_all_subnets()
        assert any(
            "all subnets" in reason
            for reason in Policy(allowed_netuids=[1, 7]).check(operation, None)
        )
        assert Policy().check(operation, None) == []
