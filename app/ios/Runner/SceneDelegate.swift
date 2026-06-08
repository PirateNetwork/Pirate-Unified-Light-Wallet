import Flutter
import UIKit

class SceneDelegate: FlutterSceneDelegate {
    override func sceneDidEnterBackground(_ scene: UIScene) {
        super.sceneDidEnterBackground(scene)
        BackgroundSyncManager.shared.scheduleCompactSync()
        BackgroundSyncManager.shared.scheduleDeepSync()
        print("[SceneDelegate] Scheduled background sync tasks")
    }

    override func sceneWillEnterForeground(_ scene: UIScene) {
        super.sceneWillEnterForeground(scene)
        UIApplication.shared.applicationIconBadgeNumber = 0
    }
}
