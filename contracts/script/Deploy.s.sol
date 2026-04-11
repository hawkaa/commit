// SPDX-License-Identifier: MIT
pragma solidity ^0.8.30;

import {Script, console} from "forge-std/Script.sol";
import {CommitAttestationRegistry} from "../src/CommitAttestationRegistry.sol";

contract DeployScript is Script {
    function run() external {
        uint256 deployerKey = vm.envUint("DEPLOYER_PRIVATE_KEY");
        vm.startBroadcast(deployerKey);

        CommitAttestationRegistry registry = new CommitAttestationRegistry();

        vm.stopBroadcast();

        console.log("CommitAttestationRegistry deployed at:", address(registry));
    }
}
