{
  pkgs,
  n2c,
  skopeo-n2c,
}:

rec {
  mkDioxusWebImage =
    {
      appName,
      bundlePath,
      imageName ? "registry.fly.io/${appName}",
      tag ? "latest",
      port ? 8080,
      entrypoint ? [ "/app/server" ],
      env ? [ ],
      extraRootPaths ? [ ],
    }:
    n2c.buildImage {
      name = imageName;
      inherit tag;
      config = {
        inherit entrypoint;
        env = [
          "IP=0.0.0.0"
          "PORT=${toString port}"
        ]
        ++ env;
        exposedPorts = {
          "${toString port}/tcp" = { };
        };
        workingdir = "/app";
      };
      copyToRoot = pkgs.buildEnv {
        name = "${appName}-image-root";
        paths = [
          pkgs.cacert
          (pkgs.runCommand "${appName}-site-bundle" { } ''
            if [ ! -d "${bundlePath}" ]; then
              echo "error: Dioxus web bundle not found at ${bundlePath}" >&2
              echo "hint: run 'dx bundle --platform web --release' and stage the output first" >&2
              exit 1
            fi

            mkdir -p "$out/app"
            cp -r "${bundlePath}/." "$out/app/"
            chmod +x "$out/app/server" || true
          '')
        ]
        ++ extraRootPaths;
        pathsToLink = [
          "/etc"
          "/app"
        ];
      };
    };

  mkDioxusWebBundleShellHook =
    {
      appName,
      dxPackageName ? "${appName}-site",
    }:
    ''
      alias site-serve='dx serve --platform web'
      alias site-build='dx build --release --platform web'
      alias site-image='DX_BUNDLE_DIR=$PWD/bundle nix build --impure .#image'
      site-bundle() {
        dx bundle --platform web --release && \
        rm -rf ./bundle && \
        cp -r target/dx/${dxPackageName}/release/web ./bundle
      }

      echo ""
      echo "  ${appName} Dioxus web deployment"
      echo "  -------------------------------------"
      echo "  site-serve              dx serve --platform web"
      echo "  site-build              dx build --release --platform web"
      echo "  site-bundle             dx bundle + stage ./bundle"
      echo "  site-image              build OCI image from ./bundle"
      echo "  nix run  .#push         push image to registry"
      echo "  nix run  .#deploy       push + flyctl deploy"
      echo ""
    '';

  mkDioxusWebDeployApps =
    {
      appName,
      imagePackage ? "image",
      registry ? "registry.fly.io",
      imageName ? appName,
      tag ? "latest",
      flyAppName ? appName,
      dxPackageName ? "${appName}-site",
    }:
    let
      imageRef = "${registry}/${imageName}:${tag}";
      defaultBundleDir = "target/dx/${dxPackageName}/release/web";
      checkBundle = ''
        : "''${DX_BUNDLE_DIR:=$PWD/${defaultBundleDir}}"
        export DX_BUNDLE_DIR
        if [ ! -x "$DX_BUNDLE_DIR/server" ]; then
          echo "error: $DX_BUNDLE_DIR/server missing - run 'dx bundle --platform web --release' first" >&2
          exit 1
        fi
      '';
    in
    {
      push = {
        type = "app";
        program = toString (
          pkgs.writeShellApplication {
            name = "push-${appName}";
            runtimeInputs = [ skopeo-n2c ];
            text = ''
              set -euo pipefail
              ${checkBundle}
              IMAGE=$(nix build --impure --no-link --print-out-paths ".#${imagePackage}")
              skopeo --insecure-policy copy \
                nix:"$IMAGE" \
                docker://${imageRef}
            '';
          }
          + "/bin/push-${appName}"
        );
      };

      deploy = {
        type = "app";
        program = toString (
          pkgs.writeShellApplication {
            name = "deploy-${appName}";
            runtimeInputs = [
              skopeo-n2c
              pkgs.flyctl
            ];
            text = ''
              set -euo pipefail
              ${checkBundle}
              IMAGE=$(nix build --impure --no-link --print-out-paths ".#${imagePackage}")
              skopeo --insecure-policy copy \
                nix:"$IMAGE" \
                docker://${imageRef}
              flyctl deploy --image ${imageRef} -a ${flyAppName}
            '';
          }
          + "/bin/deploy-${appName}"
        );
      };
    };
}
